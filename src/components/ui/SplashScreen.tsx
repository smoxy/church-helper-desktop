import { useCallback, useEffect, useState } from "react"
import { cn } from "../../lib/utils"
import { useI18n } from "../../lib/i18n"
import { RinoovaLogo } from "./RinoovaLogo"

/**
 * Single place that controls how long the splash stays fully visible before
 * it starts fading out, in milliseconds. Change this constant to change the
 * splash duration everywhere.
 */
const SPLASH_DURATION_MS = 3500

/** Length of the fade-out transition, in milliseconds. Not used at all when
 *  the user prefers reduced motion (see below). */
const FADE_DURATION_MS = 400

/**
 * Full-screen startup overlay crediting the app's sponsor. Purely
 * presentational: the only state it owns is its own visibility, driven by a
 * local timer (or an early click/Escape to skip) — no app/business logic.
 * Rendered unconditionally on every launch by App.tsx.
 */
export function SplashScreen() {
    const { t } = useI18n()
    const [visible, setVisible] = useState(true)
    const [fadingOut, setFadingOut] = useState(false)
    const [reduceMotion] = useState(
        () => window.matchMedia("(prefers-reduced-motion: reduce)").matches
    )

    const dismiss = useCallback(() => {
        if (reduceMotion) {
            // "solo timer": no fade transition, just stop showing it.
            setVisible(false)
            return
        }
        setFadingOut(true)
        window.setTimeout(() => setVisible(false), FADE_DURATION_MS)
    }, [reduceMotion])

    useEffect(() => {
        const timer = window.setTimeout(dismiss, SPLASH_DURATION_MS)
        return () => window.clearTimeout(timer)
    }, [dismiss])

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") dismiss()
        }
        window.addEventListener("keydown", onKeyDown)
        return () => window.removeEventListener("keydown", onKeyDown)
    }, [dismiss])

    if (!visible) return null

    return (
        <div
            onClick={dismiss}
            className={cn(
                "fixed inset-0 z-100 flex cursor-pointer flex-col items-center justify-center gap-10 bg-background px-8 text-center",
                !reduceMotion && "transition-opacity ease-out",
                fadingOut ? "opacity-0" : "opacity-100"
            )}
            style={reduceMotion ? undefined : { transitionDuration: `${FADE_DURATION_MS}ms` }}
        >
            <div className="flex flex-col items-center gap-3">
                <h1 className="text-5xl font-bold tracking-tight sm:text-6xl">
                    <span className="text-primary">Church</span>{" "}
                    <span className="text-foreground">Helper</span>
                </h1>
                <p className="max-w-sm text-base text-foreground/80">
                    {t('splash.subtitle')}
                </p>
            </div>

            <div className="h-px w-24 bg-border" />

            <div className="flex flex-col items-center gap-3">
                <span className="text-xs font-medium uppercase tracking-wide text-foreground/80">
                    {t('splash.sponsoredBy')}
                </span>
                <RinoovaLogo className="h-12 sm:h-14" />
            </div>
        </div>
    )
}
