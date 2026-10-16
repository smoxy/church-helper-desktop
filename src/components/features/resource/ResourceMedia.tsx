import { useState, type ReactNode } from "react";
import { BookOpen, Film, Globe, HandCoins, Users, type LucideIcon } from "lucide-react";
import { useI18n } from "../../../lib/i18n";
import { cn } from "../../../lib/utils";

interface ResourceMediaProps {
    /** Image to show (today the thumbnail; later e.g. a storyboard frame). */
    imageUrl: string | null;
    /** Alternative text of the image when it is shown. */
    alt: string;
    /** Raw category key: picks the placeholder's tint and icon. */
    category: string;
    /** Display label of the category, shown on the placeholder. */
    categoryLabel: string;
    /** Placeholder scale: compact on cards, larger in the detail modal. */
    size?: "card" | "detail";
    /** Show the category chip over a real image (the placeholder already names it). */
    showCategoryChip?: boolean;
    className?: string;
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
 * Fixed 16:9 media box shared by resource cards and the detail modal. It is
 * always rendered, so a missing image never changes the layout: without
 * `imageUrl` (or when the image fails to load) it shows a category-tinted
 * placeholder with an icon and a short label, exposed as a single image with
 * an accessible name. Kept as the one place that draws resource media so a
 * future storyboard / hover-scrub image only needs to extend this component.
 */
export function ResourceMedia({
    imageUrl,
    alt,
    category,
    categoryLabel,
    size = "card",
    showCategoryChip = false,
    className,
    children,
}: ResourceMediaProps) {
    const { t } = useI18n();
    // Remembers which URL failed, so a new URL is retried automatically.
    const [failedUrl, setFailedUrl] = useState<string | null>(null);
    const showImage = imageUrl !== null && imageUrl !== "" && failedUrl !== imageUrl;
    const { tone, Icon } = placeholderStyle(category);

    return (
        <div className={cn("relative aspect-video w-full shrink-0 overflow-hidden bg-muted", className)}>
            {showImage ? (
                <img
                    src={imageUrl}
                    alt={alt}
                    loading="lazy"
                    decoding="async"
                    onError={() => setFailedUrl(imageUrl)}
                    className="h-full w-full object-cover"
                />
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
            {showImage && showCategoryChip && (
                <div className="absolute bottom-2 left-2 max-w-[calc(100%-1rem)] truncate bg-black/70 text-white text-[10px] px-2 py-0.5 rounded-sm uppercase font-bold tracking-wider">
                    {categoryLabel}
                </div>
            )}
            {children}
        </div>
    );
}
