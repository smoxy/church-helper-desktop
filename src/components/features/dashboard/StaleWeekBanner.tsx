import { TriangleAlert } from "lucide-react";
import { WeekIdentifier } from "../../../types";
import { useI18n } from "../../../lib/i18n";

interface StaleWeekBannerProps {
    currentWeek: WeekIdentifier | null;
}

// Warning banner shown on the Dashboard when the material available locally
// belongs to a week earlier than the calendar's current week
// (AppStatus.material_week_stale). Purely presentational: the staleness
// check itself happens in the Rust backend, this component only renders
// what it's told. Not dismissible by design — it must reflect live state,
// so it can only disappear once the backend reports fresh data again.
export function StaleWeekBanner({ currentWeek }: StaleWeekBannerProps) {
    const { t } = useI18n();
    return (
        <div
            role="status"
            className="flex items-start gap-3 rounded-lg border border-amber-300 bg-amber-50 px-4 py-3 text-amber-900 dark:border-amber-800/60 dark:bg-amber-950/40 dark:text-amber-200"
        >
            <TriangleAlert className="h-5 w-5 shrink-0 text-amber-600 dark:text-amber-400" aria-hidden="true" />
            <p className="text-sm font-medium">
                {t('staleWeekBanner.title')}
                {currentWeek && (
                    t('staleWeekBanner.latestWeek', { week: `${currentWeek.year}-W${currentWeek.week_number}` })
                )}
            </p>
        </div>
    );
}
