import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Input } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import { errorCopy } from "../state/errorCopy";
import type { IntentId } from "../shell/screen";
import type { IntentFields, IntentPreview, ReviewRow } from "../types/bindings";
import "./new-intent.css";

type ParsedIntent = { fields: IntentFields; preview: IntentPreview };

/** AI proposes; Rust validates; confirmation sends the exact reviewed fields. */
export function New({ onBroadcast }: { onBroadcast: (id: IntentId) => void }) {
  const [text, setText] = useState("");
  const [parsed, setParsed] = useState<ParsedIntent | null>(null);
  const [reading, setReading] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const pending = useRef(false);
  const sendingRef = useRef(false);
  const input = useRef<HTMLDivElement | null>(null);
  const result = useRef<HTMLElement | null>(null);

  useEffect(() => () => { request.current += 1; }, []);

  function changeText(next: string) {
    // Editing invalidates both the result and any model response still in flight.
    request.current += 1;
    pending.current = false;
    setText(next);
    setParsed(null);
    setConfirming(false);
    setReading(false);
    setError(null);
  }

  async function parse() {
    if (!text.trim() || pending.current || sendingRef.current) return;
    const version = ++request.current;
    pending.current = true;
    setReading(true);
    setParsed(null);
    setError(null);
    try {
      const fields = await invoke<IntentFields>("parse_intent_chat", { text });
      if (request.current !== version) return;
      const preview = await invoke<IntentPreview>("preview_intent", { fields });
      if (request.current !== version) return;
      setParsed({ fields, preview });
      // Put the result within the visible viewport after the keyboard closes.
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      window.requestAnimationFrame(() => result.current?.focus());
    } catch (failure) {
      if (request.current === version) setError(requestError(failure));
    } finally {
      if (request.current === version) {
        pending.current = false;
        setReading(false);
      }
    }
  }

  async function broadcast() {
    if (!parsed || sendingRef.current) return;
    sendingRef.current = true;
    setSending(true);
    setError(null);
    try {
      const id = await invoke<string>("broadcast_intent", { fields: parsed.fields });
      setConfirming(false);
      onBroadcast(id);
    } catch (failure) {
      setConfirming(false);
      setError(errorCopy(failure));
    } finally {
      sendingRef.current = false;
      setSending(false);
    }
  }

  return (
    <div className="intent-composer">
      <div className="intent-composer-input" ref={input}>
        <label htmlFor="intent-chat" className="intent-composer-label">WHAT WOULD YOU LIKE TO TRADE?</label>
        <p id="intent-chat-help" className="intent-composer-help">
          Describe your trade. AI turns it into an intent for you to review.
        </p>
        <Input
          id="intent-chat"
          multiline
          rows={4}
          value={text}
          maxLength={2000}
          disabled={sending}
          aria-describedby={`intent-chat-help intent-chat-note${error ? " intent-chat-error" : ""}`}
          invalid={error !== null}
          placeholder="I want to sell 0.1 wNEAR above 3.9 USD"
          onChange={(event) => changeText((event.target as HTMLTextAreaElement).value)}
          onFocus={() => {
            window.setTimeout(() => input.current?.scrollIntoView({ block: "nearest", behavior: "smooth" }), 300);
          }}
        />
        <p id="intent-chat-note" className="intent-composer-help">
          You confirm the details before anything is sent.
        </p>
        <Button
          tone={parsed ? "secondary" : "primary"}
          size="lg"
          block
          className="cm-touch"
          disabled={reading || sending || !text.trim()}
          onClick={() => void parse()}
        >
          {reading ? "READING YOUR REQUEST…" : parsed ? "PARSE AGAIN" : "PARSE INTENT"}
        </Button>
      </div>

      <p className="intent-composer-progress" role="status" aria-live="polite">
        {reading ? "AI is reading your trade…" : parsed ? "Intent ready to review." : ""}
      </p>

      {error && <p id="intent-chat-error" role="alert" className="intent-composer-error">{error}</p>}

      {parsed && (
        <section ref={result} tabIndex={-1} aria-labelledby="parsed-intent-title" className="intent-composer-result">
          <h2 id="parsed-intent-title">YOUR INTENT</h2>
          <IntentRows rows={parsed.preview.rows} />
          <p className="intent-composer-help">To change a detail, edit your request and parse it again.</p>
          <Button tone="primary" size="lg" block className="cm-touch" disabled={sending} onClick={() => setConfirming(true)}>
            REVIEW INTENT
          </Button>
        </section>
      )}

      <ModalDialog
        open={confirming && parsed !== null}
        title="CONFIRM INTENT"
        onClose={() => { if (!sendingRef.current) setConfirming(false); }}
        footer={
          <>
            <Button tone="ghost" size="md" className="cm-touch" disabled={sending} onClick={() => setConfirming(false)}>CANCEL</Button>
            <Button tone="primary" size="md" className="cm-touch" disabled={sending} onClick={() => void broadcast()}>
              {sending ? "SENDING…" : parsed?.preview.willBroadcast ? "BROADCAST" : "QUEUE LOCALLY"}
            </Button>
          </>
        }
      >
        <div className="intent-composer-confirm">
          <IntentRows rows={parsed?.preview.rows ?? []} />
          <p className="intent-composer-help">{parsed?.preview.confirm}</p>
        </div>
      </ModalDialog>
    </div>
  );
}

function IntentRows({ rows }: { rows: ReviewRow[] }) {
  return (
    <dl className="intent-composer-rows">
      {rows.map((row) => <div key={row.key}><dt>{row.key}</dt><dd>{row.value}</dd></div>)}
    </dl>
  );
}

/** Missing terms ask for a clearer phrase; no manual form is needed. */
function requestError(failure: unknown): string {
  if (typeof failure === "object" && failure !== null && "kind" in failure && failure.kind === "invalid_intent" && "detail" in failure) {
    const detail = failure.detail as { field: string; reason: string };
    if (detail.reason === "missing") {
      const hint: Record<string, string> = {
        action: "Say whether you want to buy or sell, then parse again.",
        asset: "Include a supported token, such as wNEAR, then parse again.",
        amount: "Include how many tokens you want to trade, then parse again.",
        condition: "Include a price condition, such as above 3.9 USD or at any price, then parse again.",
        price: "Include your price in USD, then parse again.",
      };
      return hint[detail.field] ?? "Add the missing details to your request and parse again.";
    }
    if (detail.field === "request" && detail.reason === "out_of_range") return "Shorten your request and try again.";
  }
  return errorCopy(failure);
}
