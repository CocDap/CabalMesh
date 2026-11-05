#!/usr/bin/env python3
"""Generate TypeScript prop contracts for the vendored design system.

The design system shipped no .d.ts files, but its adherence lint config encodes
the same information as regexes — declared prop lists and enumerated value
domains. Turning those into types means a bad prop value is a compile error
rather than a lint warning nobody reads.

Regenerate after replacing the vendored tree:

    python3 scripts/generate-ds-types.py
"""

from __future__ import annotations

import collections
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "src" / "ds" / "_adherence.oxlintrc.json"
OUTPUT = ROOT / "src" / "ds" / "components" / "index.d.ts"

# Exported by the barrel but carrying no prop contract in the lint config.
UNCONTRACTED = [
    "CornerTicks", "LogoType", "StatInline", "RadioGroup", "ToastStack",
    "NavBar", "Tabs", "DataTable", "Terminal", "Select", "Panel", "Dialog",
    "Toast", "Switch", "Meter", "StatBlock", "Input", "Field", "Checkbox",
    "Radio", "Tooltip", "Icon", "IconButton", "Logo", "Badge", "Button",
    "Divider", "StatusDot", "TextureField", "CharacterPortrait",
]

REACT_OWNED = {"key", "ref", "className", "style", "children"}


def main() -> None:
    rules = json.loads(CONFIG.read_text())["rules"]["no-restricted-syntax"]

    props: dict[str, list[str]] = {}
    enums: dict[str, dict[str, list[str]]] = collections.defaultdict(dict)

    for rule in rules:
        if not isinstance(rule, dict):
            continue
        message = rule.get("message", "")

        declared = re.match(r"<(\w+)> doesn't accept that prop\. Declared props: (.+)\.", message)
        if declared:
            props[declared.group(1)] = [p.strip() for p in declared.group(2).split(",")]
            continue

        domain = re.match(r"<(\w+)> (\w+) must be one of (.+)\.", message)
        if domain:
            enums[domain.group(1)][domain.group(2)] = re.findall(r"'([^']+)'", domain.group(3))

    lines = [
        "// Prop contracts for the vendored design system.",
        "//",
        "// GENERATED from _adherence.oxlintrc.json by scripts/generate-ds-types.py.",
        "// Do not edit by hand — regenerate instead.",
        "",
        "import type * as React from 'react';",
        "",
        "// Every design-system component spreads `...rest` onto its root DOM",
        "// element, so ordinary DOM props are legal alongside the declared ones.",
        "// The adherence lint only enumerates the design-system props; it does not",
        "// mean the others are rejected.",
        "//",
        "// `InputHTMLAttributes` rather than `HTMLAttributes`: the base has to be",
        "// wide enough for the widest root element any component spreads onto, and",
        "// `Input`/`Select`/`Checkbox` render form controls. With the narrower base",
        "// a controlled field — `value` plus `onChange` — is a type error even",
        "// though it is exactly how these components are meant to be used. It is a",
        "// superset of `HTMLAttributes`, so nothing else loses anything.",
        "type Base = React.InputHTMLAttributes<HTMLElement> & { key?: React.Key };",
        "",
    ]

    for component in sorted(props):
        lines.append(f"export interface {component}Props extends Base {{")
        for name in props[component]:
            if name in REACT_OWNED:
                continue
            values = enums.get(component, {}).get(name)
            declared_type = " | ".join(f"'{v}'" for v in values) if values else "unknown"
            lines.append(f"  {name}?: {declared_type};")
        lines.append("}")
        lines.append(f"export declare const {component}: React.FC<{component}Props>;")
        lines.append("")

    for name in UNCONTRACTED:
        if name not in props:
            lines.append(f"export declare const {name}: React.FC<Base & Record<string, unknown>>;")

    lines += [
        "",
        "export declare const MESH_ICONS: readonly string[];",
        "export declare const CHARACTERS: readonly string[];",
    ]

    OUTPUT.write_text("\n".join(lines) + "\n")
    print(f"typed {len(props)} components, {sum(len(v) for v in enums.values())} enum props")


if __name__ == "__main__":
    main()
