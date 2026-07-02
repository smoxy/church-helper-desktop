import React from 'react';
import { CircleCheckBig, X } from 'lucide-react';

import { useCelebrationStore, Celebration } from '../../stores/celebrationStore';
import { computeSavings } from '../../lib/savings';
import { formatBytes } from '../../lib/utils';
import { RinoovaLogo } from './RinoovaLogo';

// Max panels rendered at once; the rest collapse into a "+N altri" counter
// (Intervento B, sez. 3.6).
const MAX_VISIBLE = 4;

export const CelebrationStack: React.FC = () => {
    const { celebrations, sessionSavedBytes, clearCelebrations } = useCelebrationStore();

    if (celebrations.length === 0) return null;

    return (
        <div
            role="region"
            aria-label="Risparmi Rinoova"
            aria-live="polite"
            aria-atomic="false"
            className="fixed top-4 right-4 sm:top-6 sm:right-6 z-[60] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-3 pointer-events-none"
        >
            {sessionSavedBytes > 0 && celebrations.length > 1 && (
                <div className="pointer-events-auto flex items-center justify-between rounded-md bg-card/90 backdrop-blur px-3 py-1.5 border border-border text-xs text-muted-foreground shadow">
                    <span>In questa sessione: <b className="text-success">{formatBytes(sessionSavedBytes)}</b> risparmiati</span>
                    <button onClick={clearCelebrations} className="hover:text-foreground font-medium">
                        Chiudi tutte
                    </button>
                </div>
            )}
            {celebrations.slice(0, MAX_VISIBLE).map(c => (
                <CelebrationPanel key={c.id} c={c} />
            ))}
            {celebrations.length > MAX_VISIBLE && (
                <p className="pointer-events-auto text-center text-xs text-muted-foreground">
                    +{celebrations.length - MAX_VISIBLE} altri
                </p>
            )}
        </div>
    );
};

interface CelebrationPanelProps {
    c: Celebration;
}

const CelebrationPanel: React.FC<CelebrationPanelProps> = ({ c }) => {
    const { removeCelebration } = useCelebrationStore();
    const savings = computeSavings(c.originalBytes, c.optimizedBytes);

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
                    <span className="text-sm font-bold">Download ottimizzato completato</span>
                </div>
                <button
                    onClick={() => removeCelebration(c.id)}
                    aria-label="Chiudi"
                    className="text-muted-foreground hover:text-foreground rounded-full p-1 hover:bg-muted"
                >
                    <X className="h-4 w-4" />
                </button>
            </div>
            <p className="mt-1 text-xs text-muted-foreground truncate" title={c.title}>{c.title}</p>

            {savings ? (
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
                            −{savings.percent}%
                        </span>
                        <span className="text-xs text-muted-foreground">
                            {formatBytes(c.optimizedBytes!)} invece di {formatBytes(c.originalBytes!)}
                        </span>
                    </div>
                    <p className="mt-1 text-xs text-foreground">
                        Hai risparmiato <b className="text-success">{formatBytes(savings.savedBytes)}</b>.
                    </p>
                </>
            ) : (
                <p className="mt-3 text-xs text-foreground">
                    Hai scaricato la versione leggera: stessa qualità, molto meno spazio.
                </p>
            )}

            <div className="mt-3 flex items-center gap-2 border-t border-border pt-2">
                <span className="text-[11px] text-muted-foreground">Versione ottimizzata offerta da</span>
                <RinoovaLogo className="h-3.5" />
            </div>
        </div>
    );
};
