import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Link, type LinkProps } from "react-router";
import { cx } from "../../lib/cx.ts";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md";

const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-on-accent hover:bg-accent-hover",
  secondary: "border border-border bg-surface text-fg hover:bg-hover",
  ghost: "text-fg-muted hover:bg-hover hover:text-fg",
  danger: "border border-border bg-surface text-danger hover:bg-danger-subtle",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "h-8 px-3",
  md: "h-9 px-4",
};

const ICON_ONLY: Record<ButtonSize, string> = {
  sm: "size-8",
  md: "size-9",
};

export function buttonClasses(
  variant: ButtonVariant = "secondary",
  size: ButtonSize = "md",
  iconOnly = false,
  className?: string,
): string {
  return cx(
    "inline-flex shrink-0 select-none items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium",
    "transition-colors duration-150 ease-out disabled:pointer-events-none disabled:opacity-50",
    VARIANTS[variant],
    iconOnly ? ICON_ONLY[size] : SIZES[size],
    className,
  );
}

interface CommonProps {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: LucideIcon;
  /** Renders only the icon; `label` becomes the accessible name. */
  iconOnly?: boolean;
  label?: string;
  children?: ReactNode;
}

export type ButtonProps = CommonProps & Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children">;

export function Button({
  variant,
  size,
  icon: Icon,
  iconOnly = false,
  label,
  children,
  className,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={buttonClasses(variant, size, iconOnly, className)}
      aria-label={iconOnly ? label : undefined}
      title={iconOnly ? label : undefined}
      {...rest}
    >
      {Icon && <Icon aria-hidden />}
      {iconOnly ? null : (children ?? label)}
    </button>
  );
}

export type ButtonLinkProps = CommonProps & Omit<LinkProps, "children">;

export function ButtonLink({ variant, size, icon: Icon, iconOnly = false, label, children, className, ...rest }: ButtonLinkProps) {
  return (
    <Link
      className={buttonClasses(variant, size, iconOnly, className)}
      aria-label={iconOnly ? label : undefined}
      title={iconOnly ? label : undefined}
      {...rest}
    >
      {Icon && <Icon aria-hidden />}
      {iconOnly ? null : (children ?? label)}
    </Link>
  );
}
