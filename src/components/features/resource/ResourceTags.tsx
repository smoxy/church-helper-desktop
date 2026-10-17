import type { ResourceTag } from "../../../types";
import { useI18n } from "../../../lib/i18n";
import { tagChips } from "../../../lib/mediaMeta";
import { cn } from "../../../lib/utils";

interface ResourceTagsProps {
    tags: ResourceTag[] | null | undefined;
    /** Cap on the chips shown (cards show at most 2; the modal shows all). */
    limit?: number;
    className?: string;
}

/**
 * Certain-data tag chips (number, quarter, duration). Category and date are
 * shown elsewhere, unknown facets and malformed values are skipped; renders
 * nothing when no chip is left. Each chip carries its facet name for screen
 * readers ("Trimestre: 4° trimestre 2026").
 */
export function ResourceTags({ tags, limit, className }: ResourceTagsProps) {
    const { t } = useI18n();
    const chips = tagChips(tags, limit);
    if (chips.length === 0) return null;
    return (
        <ul aria-label={t('tags.listAria')} className={cn("flex min-w-0 flex-wrap gap-1.5", className)}>
            {chips.map(chip => (
                <li
                    key={chip.facet}
                    className="max-w-full truncate rounded-full border border-border bg-muted px-2 py-0.5 text-xs font-medium text-foreground"
                >
                    <span className="sr-only">{t(chip.labelKey)}: </span>
                    {t(chip.textKey, chip.vars)}
                </li>
            ))}
        </ul>
    );
}
