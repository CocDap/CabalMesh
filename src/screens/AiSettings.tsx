import { useEffect, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Field, Input, Panel, Select } from "../ds";
import { errorCopy } from "../state/errorCopy";
import type { LlmProbe, LlmSettingsView, Preset } from "../types/bindings";

const NO_TOOLS = "THIS MODEL DOES NOT CALL TOOLS. THE AGENT WILL FETCH PRICES ITSELF.";

const noteStyle = {
  fontFamily: "var(--type-label-family)",
  fontSize: "var(--text-2xs)",
  letterSpacing: "var(--tracking-widest)",
  color: "var(--text-muted)",
} as const;

type Form = { preset: Preset; baseUrl: string; model: string; calls: string };

function formOf(view: LlmSettingsView): Form {
  return { preset: view.preset, baseUrl: view.baseUrl, model: view.model, calls: String(view.callsPerHour) };
}

function sameForm(a: Form, b: Form): boolean {
  return a.preset === b.preset && a.baseUrl === b.baseUrl && a.model === b.model && a.calls === b.calls;
}

/**
 * Which model this device talks to.
 *
 * **The key only goes one way.** It is typed here and sent to Rust, which never
 * returns it. All this screen ever learns is `hasApiKey`.
 *
 * Connection tests run against what is saved, so `TEST CONNECTION` waits until
 * the form has no unsaved changes. Otherwise it would report on settings the
 * screen is not showing.
 */
export function AiSettings() {
  const [view, setView] = useState<LlmSettingsView | null>(null);
  const [form, setForm] = useState<Form | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [probe, setProbe] = useState<LlmProbe | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    // Retried, not fetched once: the screen can mount before bootstrap has
    // published the services this command needs.
    const load = () =>
      invoke<LlmSettingsView>("llm_get_settings")
        .then((next) => {
          if (cancelled) return;
          setView(next);
          setForm(formOf(next));
        })
        .catch(() => {
          if (!cancelled) timer = window.setTimeout(load, 2_000);
        });
    load();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, []);

  if (!view || !form) return null;

  const dirty = !sameForm(form, formOf(view));
  const key = apiKey.trim();

  const accept = (next: LlmSettingsView) => {
    setView(next);
    setForm(formOf(next));
  };

  const choosePreset = (preset: Preset) => {
    const chosen = view.presets.find((option) => option.id === preset);
    if (!chosen) return;
    // The saved values when coming back to the saved preset, so a custom URL
    // survives looking at another option.
    setForm(
      preset === view.preset
        ? formOf(view)
        : { ...form, preset, baseUrl: chosen.baseUrl, model: chosen.model },
    );
    setProbe(null);
  };

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  };

  const save = (event: FormEvent) => {
    event.preventDefault();
    void run(async () => {
      if (dirty) {
        accept(
          await invoke<LlmSettingsView>("llm_save_settings", {
            preset: form.preset,
            baseUrl: form.baseUrl,
            model: form.model,
            // A blank or partial number reaches Rust as 0, which it refuses.
            callsPerHour: Number.parseInt(form.calls, 10) || 0,
          }),
        );
        setProbe(null);
      }
      if (key) {
        accept(await invoke<LlmSettingsView>("llm_set_api_key", { key }));
        setApiKey("");
      }
    });
  };

  const clearKey = () =>
    void run(async () => {
      accept(await invoke<LlmSettingsView>("llm_set_api_key", { key: "" }));
    });

  const test = () =>
    void run(async () => {
      setProbe(null);
      setProbe(await invoke<LlmProbe>("llm_test_connection"));
      accept(await invoke<LlmSettingsView>("llm_get_settings"));
    });

  const noTools = probe ? probe.jsonOk && !probe.toolsOk : !dirty && view.toolsSupported === false;

  return (
    <Panel label="AI MODEL">
      <form
        onSubmit={save}
        style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}
      >
        <Field label="PROVIDER" htmlFor="llm-preset">
          <Select
            id="llm-preset"
            value={form.preset}
            onChange={(event) => choosePreset((event.target as HTMLSelectElement).value as Preset)}
          >
            {view.presets.map((option) => (
              <option key={option.id} value={option.id}>
                {option.label}
              </option>
            ))}
          </Select>
        </Field>

        <Field label="BASE URL" htmlFor="llm-base-url">
          <Input
            id="llm-base-url"
            value={form.baseUrl}
            inputMode="url"
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            placeholder="https://host/v1"
            onChange={(event) => setForm({ ...form, baseUrl: (event.target as HTMLInputElement).value })}
          />
        </Field>

        <Field label="MODEL" htmlFor="llm-model">
          <Input
            id="llm-model"
            value={form.model}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            onChange={(event) => setForm({ ...form, model: (event.target as HTMLInputElement).value })}
          />
        </Field>

        <Field label="API KEY" htmlFor="llm-api-key">
          <Input
            id="llm-api-key"
            type="password"
            value={apiKey}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            placeholder={view.hasApiKey && !dirty ? "•••• SAVED" : "NOT SET"}
            onChange={(event) => setApiKey((event.target as HTMLInputElement).value)}
          />
        </Field>

        <Field label="MODEL CALLS PER HOUR" htmlFor="llm-calls" hint="1 TO 600">
          <Input
            id="llm-calls"
            value={form.calls}
            inputMode="numeric"
            autoComplete="off"
            onChange={(event) => setForm({ ...form, calls: (event.target as HTMLInputElement).value })}
          />
        </Field>

        {form.preset !== "ollama-local" ? (
          <span style={noteStyle}>YOUR PRICE LIMITS AND CHAT TEXT GO TO THIS ENDPOINT.</span>
        ) : null}

        {error ? (
          <span role="alert" style={{ ...noteStyle, color: "var(--accent-blood-red)" }}>
            {error}
          </span>
        ) : null}

        <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-3)" }}>
          <Button tone="primary" size="sm" className="cm-touch" type="submit" disabled={busy || (!dirty && !key)}>
            SAVE
          </Button>
          <Button tone="secondary" size="sm" className="cm-touch" type="button" disabled={busy || dirty} onClick={test}>
            {busy ? "WORKING…" : "TEST CONNECTION"}
          </Button>
          {view.hasApiKey && !dirty ? (
            <Button tone="ghost" size="sm" className="cm-touch" type="button" disabled={busy} onClick={clearKey}>
              CLEAR KEY
            </Button>
          ) : null}
        </div>

        {probe ? (
          <div role="status" style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
            <span style={noteStyle}>
              {`REACHABLE ${yesNo(probe.reachable)} · JSON ${yesNo(probe.jsonOk)} · TOOLS ${yesNo(probe.toolsOk)} · ${probe.latencyMs.toLocaleString("en-US")} MS`}
            </span>
            {probe.error ? <span style={noteStyle}>{probe.error}</span> : null}
          </div>
        ) : null}

        {noTools ? <span style={noteStyle}>{NO_TOOLS}</span> : null}
      </form>
    </Panel>
  );
}

function yesNo(value: boolean): string {
  return value ? "YES" : "NO";
}
