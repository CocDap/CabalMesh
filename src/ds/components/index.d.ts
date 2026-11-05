// Prop contracts for the vendored design system.
//
// GENERATED from _adherence.oxlintrc.json by scripts/generate-ds-types.py.
// Do not edit by hand — regenerate instead.

import type * as React from 'react';

// Every design-system component spreads `...rest` onto its root DOM
// element, so ordinary DOM props are legal alongside the declared ones.
// The adherence lint only enumerates the design-system props; it does not
// mean the others are rejected.
//
// `InputHTMLAttributes` rather than `HTMLAttributes`: the base has to be
// wide enough for the widest root element any component spreads onto, and
// `Input`/`Select`/`Checkbox` render form controls. With the narrower base
// a controlled field — `value` plus `onChange` — is a type error even
// though it is exactly how these components are meant to be used. It is a
// superset of `HTMLAttributes`, so nothing else loses anything.
type Base = React.InputHTMLAttributes<HTMLElement> & { key?: React.Key };

export interface BadgeProps extends Base {
  tone?: 'neutral' | 'quiet' | 'loud' | 'info' | 'success' | 'alert';
  size?: 'sm' | 'md';
  icon?: unknown;
}
export declare const Badge: React.FC<BadgeProps>;

export interface ButtonProps extends Base {
  tone?: 'primary' | 'secondary' | 'ghost' | 'danger' | 'signal';
  size?: 'sm' | 'md' | 'lg';
  block?: unknown;
  disabled?: unknown;
  as?: 'button' | 'a' | 'div';
  icon?: unknown;
  iconAfter?: unknown;
}
export declare const Button: React.FC<ButtonProps>;

export interface CharacterPortraitProps extends Base {
  name?: 'oracle' | 'monk' | 'watcher' | 'keeper' | 'archivist' | 'pilgrim' | 'ghost' | 'machine-priest';
  size?: unknown;
  tint?: unknown;
  label?: unknown;
  basePath?: unknown;
}
export declare const CharacterPortrait: React.FC<CharacterPortraitProps>;

export interface CheckboxProps extends Base {
  checked?: unknown;
  indeterminate?: unknown;
  label?: unknown;
  description?: unknown;
}
export declare const Checkbox: React.FC<CheckboxProps>;

export interface DataTableColumnProps extends Base {
  header?: unknown;
  align?: 'left' | 'right' | 'center';
  width?: unknown;
  emphasis?: unknown;
  render?: unknown;
}
export declare const DataTableColumn: React.FC<DataTableColumnProps>;

export interface DialogProps extends Base {
  open?: unknown;
  title?: unknown;
  onClose?: unknown;
  footer?: unknown;
  width?: unknown;
}
export declare const Dialog: React.FC<DialogProps>;

export interface DividerProps extends Base {
  ornament?: unknown;
  label?: unknown;
  vertical?: unknown;
  tone?: 'hairline' | 'default' | 'strong';
}
export declare const Divider: React.FC<DividerProps>;

export interface FieldProps extends Base {
  label?: unknown;
  hint?: unknown;
  error?: unknown;
  htmlFor?: unknown;
  required?: unknown;
}
export declare const Field: React.FC<FieldProps>;

export interface IconProps extends Base {
  name?: 'node' | 'agent' | 'intent' | 'mesh' | 'proof' | 'escrow' | 'vault' | 'reputation' | 'signal' | 'encrypt' | 'identity' | 'bridge' | 'relayer' | 'log';
  src?: unknown;
  size?: unknown;
  tint?: unknown;
  opacity?: unknown;
  basePath?: unknown;
  alt?: unknown;
}
export declare const Icon: React.FC<IconProps>;

export interface IconButtonProps extends Base {
  size?: 'sm' | 'md' | 'lg';
  tone?: 'outline' | 'bare';
  active?: unknown;
  label?: unknown;
}
export declare const IconButton: React.FC<IconButtonProps>;

export interface InputProps extends Base {
  invalid?: unknown;
  disabled?: unknown;
  prefix?: unknown;
  suffix?: unknown;
  multiline?: unknown;
  rows?: unknown;
}
export declare const Input: React.FC<InputProps>;

export interface LogoProps extends Base {
  variant?: 'primary' | 'wordmark' | 'symbol' | 'icon' | 'minimal' | 'hero' | 'emblem';
  size?: unknown;
  tint?: unknown;
  basePath?: unknown;
  title?: unknown;
}
export declare const Logo: React.FC<LogoProps>;

export interface MeterProps extends Base {
  value?: unknown;
  max?: unknown;
  tone?: 'white' | 'info' | 'success' | 'alert' | 'quiet';
  size?: unknown;
  label?: unknown;
  readout?: unknown;
  segments?: unknown;
}
export declare const Meter: React.FC<MeterProps>;

export interface NavBarItemProps extends Base {
  id?: unknown;
  label?: unknown;
}
export declare const NavBarItem: React.FC<NavBarItemProps>;

export interface PanelProps extends Base {
  label?: unknown;
  action?: unknown;
  ticks?: unknown;
  sheen?: unknown;
  pad?: unknown;
  tone?: 'panel' | 'void' | 'raised' | 'none';
  border?: 'hairline' | 'default' | 'strong' | 'loud' | 'none';
  bodyStyle?: unknown;
}
export declare const Panel: React.FC<PanelProps>;

export interface RadioProps extends Base {
  checked?: unknown;
  label?: unknown;
  description?: unknown;
}
export declare const Radio: React.FC<RadioProps>;

export interface SelectOptionProps extends Base {
  value?: unknown;
  label?: unknown;
}
export declare const SelectOption: React.FC<SelectOptionProps>;

export interface StatBlockProps extends Base {
  label?: unknown;
  value?: unknown;
  unit?: unknown;
  delta?: unknown;
  deltaTone?: 'up' | 'down' | 'neutral';
  size?: 'sm' | 'md' | 'lg';
}
export declare const StatBlock: React.FC<StatBlockProps>;

export interface StatusDotProps extends Base {
  tone?: 'online' | 'alert' | 'info' | 'idle' | 'offline';
  size?: unknown;
  pulse?: unknown;
  glow?: unknown;
  label?: unknown;
}
export declare const StatusDot: React.FC<StatusDotProps>;

export interface SwitchProps extends Base {
  checked?: unknown;
  label?: unknown;
  showState?: unknown;
}
export declare const Switch: React.FC<SwitchProps>;

export interface TabItemProps extends Base {
  id?: unknown;
  label?: unknown;
  icon?: unknown;
  count?: unknown;
}
export declare const TabItem: React.FC<TabItemProps>;

export interface TerminalLineProps extends Base {
  text?: unknown;
  tone?: 'out' | 'dim' | 'ok' | 'err' | 'info' | 'loud';
  prompt?: unknown;
}
export declare const TerminalLine: React.FC<TerminalLineProps>;

export interface TextureFieldProps extends Base {
  grid?: unknown;
  gridScale?: 'fine' | 'coarse';
  scanlines?: unknown;
  vignette?: unknown;
  dither?: unknown;
  glitch?: unknown;
  opacity?: unknown;
  basePath?: unknown;
}
export declare const TextureField: React.FC<TextureFieldProps>;

export interface ToastProps extends Base {
  tone?: 'neutral' | 'info' | 'success' | 'alert';
  title?: unknown;
  icon?: unknown;
  onClose?: unknown;
}
export declare const Toast: React.FC<ToastProps>;

export interface TooltipProps extends Base {
  label?: unknown;
  placement?: 'top' | 'bottom' | 'left' | 'right';
}
export declare const Tooltip: React.FC<TooltipProps>;

export declare const CornerTicks: React.FC<Base & Record<string, unknown>>;
export declare const LogoType: React.FC<Base & Record<string, unknown>>;
export declare const StatInline: React.FC<Base & Record<string, unknown>>;
export declare const RadioGroup: React.FC<Base & Record<string, unknown>>;
export declare const ToastStack: React.FC<Base & Record<string, unknown>>;
export declare const NavBar: React.FC<Base & Record<string, unknown>>;
export declare const Tabs: React.FC<Base & Record<string, unknown>>;
export declare const DataTable: React.FC<Base & Record<string, unknown>>;
export declare const Terminal: React.FC<Base & Record<string, unknown>>;
export declare const Select: React.FC<Base & Record<string, unknown>>;

export declare const MESH_ICONS: readonly string[];
export declare const CHARACTERS: readonly string[];
