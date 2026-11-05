/**
 * The design system's public entry.
 *
 * Screens import from here, never from component internals — the adherence
 * lint enforces that, and it is what lets the vendored tree be replaced
 * wholesale when the design system is regenerated.
 */
import "./styles.css";
import "./mobile.css";

export * from "./components/index.js";
