import { Switch } from "../../ui/switch";
import { Savings } from "../../../lib/savings";

interface OptimizedPreferenceToggleProps {
    savings: Savings | null;
    originalLabel: string | null;
    optimizedLabel: string | null;
    onEnable: () => void;
}

// Presentational-only (UI-dumb guard): no calculation happens here, callers
// pass an already-computed `savings` (see lib/savings.ts::computeSavings).
export function OptimizedPreferenceToggle({ savings, originalLabel, optimizedLabel, onEnable }: OptimizedPreferenceToggleProps) {
    return (
        <div className="flex items-start gap-3 rounded-lg border border-primary/30 bg-primary/5 p-3">
            <div className="min-w-0 flex-1">
                <p className="text-sm font-semibold text-foreground">Passa alla versione ottimizzata</p>
                {savings ? (
                    <p className="mt-0.5 text-xs text-muted-foreground">
                        Stessa qualità, molto più leggera:{" "}
                        <span className="font-semibold text-success">−{savings.percent}%</span>{" "}
                        — {optimizedLabel} invece di {originalLabel}.
                    </p>
                ) : (
                    <p className="mt-0.5 text-xs text-muted-foreground">
                        Stessa qualità percepita, ma pesa fino a 10 volte di meno.
                    </p>
                )}
            </div>
            <Switch
                checked={false}
                onCheckedChange={(c) => { if (c) onEnable(); }}
                aria-label="Preferisci sempre i video ottimizzati"
                className="mt-0.5 shrink-0"
            />
        </div>
    );
}
