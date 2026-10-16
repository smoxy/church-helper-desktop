import { Resource } from "../../../types";
import { useI18n } from "../../../lib/i18n";
import { ResourceCard } from "../resource/ResourceCard";

interface NextWeekSectionProps {
    resources: Resource[];
    autoDownloadEnabled: boolean;
    onSelect: (resource: Resource) => void;
}

// Dashboard section for next week's material (GET /api/resources/next-week,
// fetched best-effort by the backend on every poll). Purely presentational:
// each ResourceCard shows its own download state (downloaded / queued /
// downloading), exactly like the current week's cards.
export function NextWeekSection({ resources, autoDownloadEnabled, onSelect }: NextWeekSectionProps) {
    const { t } = useI18n();
    return (
        <section className="space-y-4" aria-labelledby="next-week-title">
            <div className="space-y-1">
                <h3 id="next-week-title" className="text-xl font-semibold">{t('dashboard.nextWeek.title')}</h3>
                <p className="text-sm text-muted-foreground">
                    {autoDownloadEnabled ? t('dashboard.nextWeek.autoOn') : t('dashboard.nextWeek.autoOff')}
                </p>
            </div>
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
                {resources.length === 0 ? (
                    <div className="col-span-full text-center py-10 border rounded-lg bg-card text-muted-foreground">
                        {t('dashboard.nextWeek.empty')}
                    </div>
                ) : (
                    resources.map((resource) => (
                        <ResourceCard
                            key={resource.id}
                            resource={resource}
                            onClick={() => onSelect(resource)}
                        />
                    ))
                )}
            </div>
        </section>
    );
}
