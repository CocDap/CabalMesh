import type { AppError, ModelProblem } from "../types/bindings";

/**
 * Brand-voice copy for a typed error.
 *
 * Rust sends a **variant**, never a sentence — see `src-tauri/src/error.rs` for
 * why: `e.to_string()` leaks filesystem paths and RPC endpoints into the
 * webview, and prose cannot be branched on, so on-voice copy would be
 * impossible. The variant is the contract; this file is the sentence.
 *
 * Copy rules the strings here follow: declarative, present tense, full stops,
 * no apology, no exclamation, and never "Something went wrong."
 */
export function errorCopy(failure: unknown): string {
  const error = asAppError(failure);
  if (!error) return "The request was refused. Nothing was sent.";

  switch (error.kind) {
    case "not_ready":
      return "Still connecting. Nothing has been sent.";
    case "unsupported":
      return "This device cannot do that.";
    case "mesh_offline":
      return "Mesh unreachable. Queued locally.";
    case "invalid_intent":
      return invalidCopy(error.detail.field, error.detail.reason);
    case "chain":
      return error.detail.retryable
        ? "The chain did not answer. Nothing settled. Try again."
        : "The chain rejected it. Nothing settled.";
    case "near_account_not_funded":
      return `FUND THIS ACCOUNT WITH NEAR FIRST: ${error.detail.account}`;
    case "insufficient_native":
      return "NEAR must cover the amount, account storage and network fees. Reduce the amount or add NEAR.";
    case "native_transfer_unconfirmed":
      return "Transfer not confirmed. Refresh balances before retrying. Wrapped NEAR in your wallet can be withdrawn as NEAR.";
    case "vault_locked":
      return "The vault is locked. No key is available.";
    case "too_many_subscriptions":
      return "Too many live streams. Close a screen and retry.";
    case "model":
      return modelCopy(error.detail.problem);
    case "internal":
      return "The request was refused. Nothing was sent.";
  }
}

/** What to fix, and where. Every one of these is fixed in Profile. */
function modelCopy(problem: ModelProblem): string {
  switch (problem) {
    case "unreachable":
      return "The model did not answer. Check the base URL in Profile.";
    case "refused":
      return "The model endpoint refused. Check the key and model in Profile.";
    case "bad_answer":
      return "The model's answer could not be read. Fill the form by hand.";
    case "budget_used_up":
      return "The hourly AI call limit is used up. Raise it in Profile.";
    case "no_model":
      return "No model is set. Choose one in Profile.";
  }
}

/** Field-specific copy, so a form can attach the failure to the input. */
function invalidCopy(field: string, reason: AppErrorReason): string {
  const name = field.toUpperCase();
  switch (reason) {
    case "missing":
      return `${name} is required.`;
    case "malformed":
      return `${name} is not a value this accepts.`;
    case "out_of_range":
      return `${name} is outside what this allows.`;
    case "too_precise":
      return `${name} carries more decimals than this asset has.`;
    case "insecure":
      return `${name} must use https to carry a key.`;
    case "insufficient_funds":
      return "Balance does not cover this intent.";
    default:
      return `${name} was refused.`;
  }
}

type AppErrorReason = Extract<AppError, { kind: "invalid_intent" }>["detail"]["reason"];

/**
 * Narrows an unknown rejection to the typed union.
 *
 * Tauri rejects with whatever the command returned, so this is an `AppError` in
 * practice — but a transport failure rejects with something else entirely, and
 * treating that as an `AppError` would read a `kind` off `undefined`.
 */
function asAppError(failure: unknown): AppError | null {
  if (typeof failure !== "object" || failure === null) return null;
  return "kind" in failure ? (failure as AppError) : null;
}
