import { Resource } from "../../../types";
import { ResourceCard } from "./ResourceCard";

interface ResourceGridProps {
    resources: Resource[];
    emptyMessage: string;
    onSelect: (resource: Resource) => void;
}

// Shared grid for the Dashboard's resource sections (current and next week).
// auto-fill (not auto-fit) keeps every card at the same track width even with
// only one or two resources, instead of stretching them across the row; grid
// rows stretch items, so cards in a row also share the same height.
// Minimum card width is 14rem, so the default 800px window (about 480px of
// content beside the sidebar) still fits two columns (min(100%,...) lets a
// narrower container fit one card); the 16:9 media box and body have fixed minimum heights in
// ResourceCard, so missing thumbnail/description/date never collapse a card.
export function ResourceGrid({ resources, emptyMessage, onSelect }: ResourceGridProps) {
    return (
        <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(min(100%,14rem),1fr))]">
            {resources.length === 0 ? (
                <div className="col-span-full text-center py-10 border rounded-lg bg-card text-muted-foreground">
                    {emptyMessage}
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
    );
}
