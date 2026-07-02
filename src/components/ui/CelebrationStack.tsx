import React from 'react';
import { CircleCheckBig, X } from 'lucide-react';

import { useCelebrationStore, Celebration } from '../../stores/celebrationStore';
import { formatBytes } from '../../lib/utils';
import { useI18n } from '../../lib/i18n';
import { RinoovaLogo } from './RinoovaLogo';

// Max panels rendered at once; the rest collapse into a "+N altri" counter
// (Intervento B, sez. 3.6).
const MAX_VISIBLE = 4;

export const CelebrationStack: React.FC = () => {
    const { t } = useI18n();
    const { celebrations, sessionSavedBytes, totalSavedBytes, clearCelebrations } = useCelebrationStore();

    if (celebrations.length === 0) return null;

    return (
        <div
            role="region"
            aria-label={t('celebration.regionAriaLabel')}
            aria-live="polite"
            aria-atomic="false"
            className="fixed top-4 right-4 sm:top-6 sm:right-6 z-[60] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-3 pointer-events-none"
        >
            {/* Always shown once there's at least one celebration (see the
                early return above): formatBytes(0) is a fine initial state,
                it updates live as savings-resolved events land — gating on
                totals > 0 used to hide the header entirely for a resource
                whose savings hadn't resolved yet. */}
            <div className="pointer-events-auto flex items-center justify-between rounded-md bg-card/90 backdrop-blur px-3 py-1.5 border border-border text-xs text-muted-foreground shadow">
                <span>
                    {t('celebration.sessionSavedPrefix')}<b className="text-success">{formatBytes(sessionSavedBytes)}</b>{t('celebration.sessionSavedSuffix')}
                    <span className="mx-1 text-muted-foreground/50">•</span>
                    {t('celebration.totalSavedPrefix')}<b className="text-foreground">{formatBytes(totalSavedBytes)}</b>{t('celebration.totalSavedSuffix')}
                </span>
                <button onClick={clearCelebrations} className="hover:text-foreground font-medium shrink-0 ml-2">
                    {t('celebration.closeAll')}
                </button>
            </div>
            {celebrations.slice(0, MAX_VISIBLE).map(c => (
                <CelebrationPanel key={c.id} c={c} />
            ))}
            {celebrations.length > MAX_VISIBLE && (
                <p className="pointer-events-auto text-center text-xs text-muted-foreground">
                    {t(
                        celebrations.length - MAX_VISIBLE === 1 ?
                            'celebration.andMoreOne' :
                            'celebration.andMoreMany',
                        { count: celebrations.length - MAX_VISIBLE })}
                </p>
            )}
        </div>
    );
};

interface CelebrationPanelProps {
    c: Celebration;
}

const CelebrationPanel: React.FC<CelebrationPanelProps> = ({ c }) => {
    const { t } = useI18n();
    const { removeCelebration } = useCelebrationStore();
    // Sourced straight from the download-complete payload snapshotted at
    // add-time (see celebrationStore) — no recomputation here. Rare now that
    // the backend does a best-effort HEAD before emitting the event, but
    // still falls back to the generic copy below when a size stayed unknown.
    const hasSavings =
        c.savedBytes !== null && c.originalBytes !== null &&
        c.optimizedBytes !== null;

    return (
        <div className="celebrate-enter pointer-events-auto rounded-xl border border-border bg-card text-card-foreground shadow-2xl ring-1 ring-success/25 p-4 relative overflow-hidden">
            {/* Decorative sparks: purely visual, disabled under reduced-motion via CSS. */}
            <div aria-hidden className="absolute right-6 top-6 pointer-events-none">
                {[...Array(5)].map((_, i) => (
                    <span
                        key={i}
                        className="celebrate-spark absolute block h-1.5 w-1.5 rounded-full bg-success"
                        style={{
                            '--spark-x': `${(i - 2) * 10}px`,
                            '--spark-y': `${-14 - i * 3}px`,
                            animationDelay: `${i * 70}ms`,
                        } as React.CSSProperties}
                    />
                ))}
            </div>
            <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2 text-success">
                    <CircleCheckBig className="h-5 w-5" />
                    <span className="text-sm font-bold">{t('celebration.title')}</span>
                </div>
                <button
                    onClick={() => removeCelebration(c.id)}
                    aria-label={t('celebration.close')}
                    className="text-muted-foreground hover:text-foreground rounded-full p-1 hover:bg-muted"
                >
                    <X className="h-4 w-4" />
                </button>
            </div>
            <p className="mt-1 text-xs text-muted-foreground truncate" title={c.title}>{c.title}</p>

            {hasSavings ? (
                <>
                    {/* Track = original weight, fill = optimized weight. */}
                    <div className="mt-3 h-2 w-full rounded-full bg-muted overflow-hidden" aria-hidden>
                        <div
                            className="celebrate-bar-fill h-full rounded-full bg-success"
                            style={{ '--opt-scale': (c.optimizedBytes! / c.originalBytes!).toFixed(3) } as React.CSSProperties}
                        />
                    </div>
                    <div className="mt-2 flex items-center justify-between">
                        <span className="rounded-full bg-success px-2 py-0.5 text-xs font-bold text-success-foreground">
                            −{c.percent}%
                        </span>
                        <span className="text-xs text-muted-foreground">
                            {t('celebration.sizeComparison', { optimizedLabel: formatBytes(c.optimizedBytes!), originalLabel: formatBytes(c.originalBytes!) })}
                        </span>
                    </div>
                    <p className="mt-1 text-xs text-foreground">
                        {t('celebration.savedAmountPrefix')}<b className="text-success">{formatBytes(c.savedBytes!)}</b>{t('celebration.savedAmountSuffix')}
                    </p>
                </>
            ) : (
                <p className="mt-3 text-xs text-foreground">
                    {t('celebration.noSavingsInfo')}
                </p>
            )}

            <div className="mt-3 flex items-center gap-2 border-t border-border pt-2">
                <span className="text-[11px] text-muted-foreground">{t('celebration.providedBy')}</span>
                <RinoovaLogo className="h-3.5" />
            </div>
        </div>
    );
};
