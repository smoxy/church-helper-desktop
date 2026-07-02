import * as React from "react"
import { cn } from "../../lib/utils"
import logoLight from "../../assets/sponsor/logo-rinoova-horizontal.svg"
import logoDark from "../../assets/sponsor/logo-rinoova-horizontal-dark.svg"

export type RinoovaLogoProps = React.ImgHTMLAttributes<HTMLImageElement>

/**
 * Rinoova horizontal logo lockup (icon + wordmark).
 *
 * The source artwork ships a near-black wordmark that vanishes on a dark
 * surface, so a second dark-safe (near-white) variant is bundled. Both are
 * rendered and the active `.dark` root class (driven by lib/theme) toggles
 * which one is visible via Tailwind's `dark:` variant — no runtime prop or
 * `prefers-color-scheme` guess, so the wordmark always matches the real theme.
 */
export const RinoovaLogo = React.forwardRef<HTMLImageElement, RinoovaLogoProps>(
    ({ className, alt = "Rinoova", ...props }, ref) => (
        <>
            <img
                ref={ref}
                src={logoLight}
                alt={alt}
                className={cn("h-full w-auto dark:hidden", className)}
                {...props}
            />
            <img
                src={logoDark}
                alt={alt}
                className={cn("hidden h-full w-auto dark:block", className)}
                {...props}
            />
        </>
    )
)
RinoovaLogo.displayName = "RinoovaLogo"
