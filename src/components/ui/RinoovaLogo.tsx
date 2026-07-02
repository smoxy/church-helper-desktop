import * as React from "react"
import { cn } from "../../lib/utils"
import logoLight from "../../assets/sponsor/logo-rinoova-horizontal.svg"
import logoDark from "../../assets/sponsor/logo-rinoova-horizontal-dark.svg"

export interface RinoovaLogoProps extends React.ImgHTMLAttributes<HTMLImageElement> {
    /**
     * Which colour variant to render, matching the surface this logo is
     * placed on:
     * - "light" (default): near-black wordmark, for a light/white surface.
     * - "dark": wordmark recoloured near-white, for a dark surface.
     *
     * The source artwork only ships the near-black wordmark, which
     * disappears on a dark surface (the brand gradient icon is the
     * opposite: legible on dark, washed out on light) — hence the second
     * bundled variant. There is deliberately no automatic OS-based
     * detection here: this app's `bg-*`/`text-*` design tokens are pinned
     * to their light `:root` values and are never switched to `.dark`
     * (only a handful of unrelated components use the OS-driven `dark:`
     * Tailwind variant directly), so picking a variant from
     * `prefers-color-scheme` would pick the dark-safe wordmark while the
     * real surface stays light, making it invisible instead of fixing it.
     * Callers pass the variant that matches their actual background.
     */
    variant?: "light" | "dark"
}

/** Rinoova horizontal logo lockup (icon + wordmark). */
export const RinoovaLogo = React.forwardRef<HTMLImageElement, RinoovaLogoProps>(
    ({ className, variant = "light", alt = "Rinoova", ...props }, ref) => (
        <img
            ref={ref}
            src={variant === "dark" ? logoDark : logoLight}
            alt={alt}
            className={cn("h-full w-auto", className)}
            {...props}
        />
    )
)
RinoovaLogo.displayName = "RinoovaLogo"
