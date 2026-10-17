import { useEffect, useState, type KeyboardEvent, type PointerEvent, type ReactNode, type Ref } from "react";
import { BookOpen, Film, Globe, HandCoins, Users, type LucideIcon } from "lucide-react";
import { useI18n } from "../../../lib/i18n";
import { cn } from "../../../lib/utils";
import { formatDuration, frameIndexAt, nextKeyFrame, spriteFrameStyle } from "../../../lib/mediaMeta";
import type { Storyboard } from "../../../types";

interface ResourceMediaProps {
    /** Poster image (thumbnail_url); null/absent or failing -> fallback. */
    imageUrl: string | null;
    /** Hover-scrub sprite; its first frame is the poster when imageUrl is missing. */
    storyboard?: Storyboard | null;
    /** Video duration in seconds, shown as m:ss on the corner when known. */
    durationSeconds?: number | null;
    /**
     * Frame to show, driven by the parent (cards: the stretched button over
     * the card captures the pointer, so the card maps the pointer position).
     * null/absent shows the poster. Ignored when `interactive`.
     */
    activeFrame?: number | null;
    /**
     * Detail modal: the box itself scrubs on mouse/pen hover and, when
     * focused, with the left/right arrow keys (role slider, named after
     * `title`). Only when a usable storyboard is present.
     */
    interactive?: boolean;
    /** Resource title, for the accessible name of the interactive box. */
    title?: string;
    /** Raw category key: picks the placeholder's tint and icon. */
    category: string;
    /** Display label of the category, shown on the placeholder. */
    categoryLabel: string;
    /** Placeholder scale: compact on cards, larger in the detail modal. */
    size?: "card" | "detail";
    /** Show the category chip over a real image (the placeholder already names it). */
    showCategoryChip?: boolean;
    className?: string;
    /** The media box element (cards map pointer positions against it). */
    ref?: Ref<HTMLDivElement>;
    /** Overlays (status badges, category chip) layered over the media. */
    children?: ReactNode;
}


interface Tone {
    bg: string;
    icon: string;
}

// Literal class strings (Tailwind only emits classes it can see in source).
// Icon colours are >= 3:1 against their own tint in both themes (WCAG 1.4.11);
// the label uses text-foreground, well above 4.5:1 on every tint.
const TONES: readonly Tone[] = [
    { bg: "bg-amber-100 dark:bg-amber-400/15", icon: "text-amber-700 dark:text-amber-300" },
    { bg: "bg-emerald-100 dark:bg-emerald-400/15", icon: "text-emerald-700 dark:text-emerald-300" },
    { bg: "bg-violet-100 dark:bg-violet-400/15", icon: "text-violet-700 dark:text-violet-300" },
    { bg: "bg-sky-100 dark:bg-sky-400/15", icon: "text-sky-700 dark:text-sky-300" },
    { bg: "bg-rose-100 dark:bg-rose-400/15", icon: "text-rose-700 dark:text-rose-300" },
    { bg: "bg-teal-100 dark:bg-teal-400/15", icon: "text-teal-700 dark:text-teal-300" },
];

// Known categories get a stable tone + a meaningful icon; any other category
// gets a tone derived from its name (stable across renders) and a generic icon.
const KNOWN: Readonly<Record<string, { tone: number; icon: LucideIcon }>> = {
    decime: { tone: 0, icon: HandCoins },
    missioni: { tone: 1, icon: Globe },
    vga: { tone: 2, icon: Users },
    scuola: { tone: 3, icon: BookOpen },
};

function placeholderStyle(category: string): { tone: Tone; Icon: LucideIcon } {
    const key = category.toLowerCase();
    const known = Object.prototype.hasOwnProperty.call(KNOWN, key) ? KNOWN[key] : undefined;
    if (known) return { tone: TONES[known.tone], Icon: known.icon };
    let hash = 0;
    for (const ch of key) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
    return { tone: TONES[hash % TONES.length], Icon: Film };
}


/**
 * Load state of a sprite URL, probed once with an Image() (the same request
 * the CSS background then reuses from cache), so a broken sprite falls back
 * to the poster instead of an empty box. Probed only once `wanted`.
 */
function useSpriteState(url: string | null, wanted: boolean): "pending" | "loaded" | "failed" {
    const [state, setState] = useState<{ url: string; ok: boolean } | null>(null);
    const probed = state?.url === url;
    useEffect(() => {
        if (!url || !wanted || probed) return;
        let alive = true;
        const img = new Image();
        img.onload = () => { if (alive) setState({ url, ok: true }); };
        img.onerror = () => { if (alive) setState({ url, ok: false }); };
        img.src = url;
        return () => { alive = false; };
    }, [url, wanted, probed]);
    if (!url || !state || !probed) return "pending";
    return state.ok ? "loaded" : "failed";
}

/**
 * Fixed 16:9 media box shared by resource cards and the detail modal. It is
 * always rendered, so a missing image never changes the layout. Layers: the
 * poster (`imageUrl`; else the storyboard's first frame; else a
 * category-tinted placeholder exposed as a single image with an accessible
 * name), then, while scrubbing, the storyboard frame under the pointer (one
 * sprite request; frames are picked with background-position). The poster
 * itself is decorative (alt=""): cards and the modal already name the
 * resource. Touch input never scrubs.
 */
export function ResourceMedia({
    imageUrl,
    storyboard = null,
    durationSeconds = null,
    activeFrame = null,
    interactive = false,
    title = "",
    category,
    categoryLabel,
    size = "card",
    showCategoryChip = false,
    className,
    ref,
    children,
}: ResourceMediaProps) {
    const { t } = useI18n();
    // Remembers which URL failed, so a new URL is retried automatically.
    const [failedUrl, setFailedUrl] = useState<string | null>(null);
    const showPoster = imageUrl !== null && imageUrl !== "" && failedUrl !== imageUrl;
    const { tone, Icon } = placeholderStyle(category);

    // Own scrubbing state, used only in interactive (detail) mode.
    const [pointerFrame, setPointerFrame] = useState<number | null>(null);
    const [keyFrame, setKeyFrame] = useState<number | null>(null);
    const requestedFrame = interactive ? keyFrame ?? pointerFrame : activeFrame;

    // The sprite is fetched only when needed: as the poster (no imageUrl),
    // in the detail modal, or once scrubbing starts on a card.
    const sprite = useSpriteState(
        storyboard?.url ?? null,
        storyboard !== null && (!showPoster || interactive || requestedFrame !== null),
    );
    const spriteOk = storyboard !== null && sprite === "loaded";
    const scrubFrame = spriteOk ? requestedFrame : null;
    const showImage = showPoster || spriteOk;
    const duration = formatDuration(durationSeconds);
    const frames = storyboard?.frames ?? 0;

    const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
        if (!storyboard || e.pointerType === "touch") return;
        const rect = e.currentTarget.getBoundingClientRect();
        if (rect.width <= 0) return;
        // The last input wins: moving the mouse hands control back to hover.
        setKeyFrame(null);
        setPointerFrame(frameIndexAt((e.clientX - rect.left) / rect.width, storyboard.frames));
    };
    const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
        if (!storyboard) return;
        const next = nextKeyFrame(e.key, keyFrame ?? pointerFrame, storyboard.frames);
        if (next === null) return;
        e.preventDefault();
        setKeyFrame(next);
    };

    // Interactive only with a sprite that is not known to be broken.
    const scrubbable = interactive && storyboard !== null && sprite !== "failed";
    const shownIndex = scrubFrame ?? 0;
    const time = storyboard && storyboard.timestamps.length === storyboard.frames
        ? formatDuration(storyboard.timestamps[shownIndex]) ?? "0:00"
        : null;
    const sliderProps = scrubbable ? {
        role: "slider",
        tabIndex: 0,
        "aria-label": t('resourceMedia.storyboardAria', { title }),
        "aria-valuemin": 1,
        "aria-valuemax": frames,
        "aria-valuenow": shownIndex + 1,
        "aria-valuetext": time
            ? t('resourceMedia.frameAt', { index: shownIndex + 1, total: frames, time })
            : t('resourceMedia.frame', { index: shownIndex + 1, total: frames }),
        onKeyDown,
        onBlur: () => setKeyFrame(null),
        onPointerMove,
        onPointerLeave: () => setPointerFrame(null),
    } : {};

    // A frame keeps the sprite's own aspect ratio (letterboxed on black), so
    // a non-16:9 video is never stretched.
    const frameLayer = (index: number) => storyboard && (
        <div aria-hidden="true" className="absolute inset-0 flex items-center justify-center bg-black">
            <div
                className={storyboard.frame_width / storyboard.frame_height >= 16 / 9 ? "w-full" : "h-full"}
                style={{
                    aspectRatio: `${storyboard.frame_width} / ${storyboard.frame_height}`,
                    ...spriteFrameStyle(storyboard, index),
                }}
            />
        </div>
    );

    return (
        <div
            ref={ref}
            {...sliderProps}
            className={cn(
                "relative aspect-video w-full shrink-0 overflow-hidden bg-muted",
                scrubbable && "cursor-ew-resize focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card",
                className,
            )}
        >
            {showPoster ? (
                <img
                    src={imageUrl}
                    alt=""
                    loading="lazy"
                    decoding="async"
                    onError={() => setFailedUrl(imageUrl)}
                    className="h-full w-full object-cover"
                />
            ) : spriteOk ? (
                frameLayer(0)
            ) : (
                <div
                    role="img"
                    aria-label={t('resourceMedia.placeholderAria', { category: categoryLabel })}
                    className={cn("flex h-full w-full flex-col items-center justify-center gap-2 px-4 text-center", tone.bg)}
                >
                    <Icon
                        aria-hidden="true"
                        className={cn(size === "detail" ? "h-12 w-12" : "h-9 w-9", tone.icon)}
                        strokeWidth={1.75}
                    />
                    <span className={cn("font-semibold text-foreground line-clamp-1 first-letter:uppercase", size === "detail" ? "text-base" : "text-sm")}>
                        {categoryLabel}
                    </span>
                    <span className="text-xs text-foreground/80">
                        {t('resourceMedia.noPreview')}
                    </span>
                </div>
            )}
            {scrubFrame !== null && frameLayer(scrubFrame)}
            {scrubFrame !== null && frames > 1 && (
                <div aria-hidden="true" className="absolute inset-x-0 bottom-0 h-1 bg-white/25">
                    <div
                        className="h-full bg-primary"
                        style={{ width: `${((scrubFrame + 1) / frames) * 100}%` }}
                    />
                </div>
            )}
            {showImage && showCategoryChip && (
                <div className={cn(
                    "absolute bottom-2 left-2 truncate bg-black/70 text-white text-[10px] px-2 py-0.5 rounded-sm uppercase font-bold tracking-wider",
                    duration ? "max-w-[calc(100%-5rem)]" : "max-w-[calc(100%-1rem)]",
                )}>
                    {categoryLabel}
                </div>
            )}
            {duration && (
                <div className="absolute bottom-2 right-2 rounded-sm bg-black/75 px-1.5 py-0.5 text-[11px] font-semibold tabular-nums text-white">
                    <span className="sr-only">{t('resourceMedia.duration', { duration })}</span>
                    <span aria-hidden="true">{duration}</span>
                </div>
            )}
            {children}
        </div>
    );
}
