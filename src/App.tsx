import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import { Resource, ResourceListResponse } from "./types";
import { ResourceDetail } from "./components/features/resource/ResourceDetail";

function App() {
  const [resources, setResources] = useState<Resource[]>([]);
  const [selectedResource, setSelectedResource] = useState<Resource | null>(null);
  const [loading, setLoading] = useState(false);

  const fetchResources = async () => {
    try {
      const data = await invoke<Resource[]>("get_resources");
      setResources(data);
    } catch (error) {
      console.error("Failed to fetch resources:", error);
    }
  };

  const handleForcePoll = async () => {
    setLoading(true);
    try {
      await invoke<ResourceListResponse>("force_poll");
      // The event listener will update the list, but we can also update manually
      await fetchResources();
    } catch (error) {
      console.error("Poll failed:", error);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchResources();

    // Listen for updates from backend
    const unlisten = listen<ResourceListResponse>("resources-updated", (event) => {
      setResources(event.payload.resources);
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  return (
    <div className="min-h-screen bg-background text-foreground p-8 font-sans">
      <header className="flex justify-between items-center mb-8">
        <div>
          <h1 className="text-3xl font-bold text-primary">Church Helper</h1>
          <p className="text-muted-foreground">Weekly Resources Dashboard</p>
        </div>
        <button
          onClick={handleForcePoll}
          disabled={loading}
          className={`
            bg-primary text-primary-foreground px-4 py-2 rounded-lg font-medium transition-colors
            ${loading ? "opacity-50 cursor-not-allowed" : "hover:bg-primary/90"}
          `}
        >
          {loading ? "Refreshing..." : "Check for Updates"}
        </button>
      </header>

      <main>
        {resources.length === 0 ? (
          <div className="text-center py-20 text-muted-foreground">
            <p className="text-lg">No resources found.</p>
            <p className="text-sm">Click "Check for Updates" to fetch the latest resources.</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6">
            {resources.map((resource) => (
              <div
                key={resource.id}
                onClick={() => setSelectedResource(resource)}
                className="bg-card text-card-foreground rounded-xl shadow-sm border border-border overflow-hidden cursor-pointer hover:shadow-md transition-shadow hover:border-primary/50 group"
              >
                <div className="aspect-video relative bg-muted">
                  {resource.thumbnail_url ? (
                    <img
                      src={resource.thumbnail_url}
                      alt={resource.title}
                      className="w-full h-full object-cover transition-transform group-hover:scale-105"
                    />
                  ) : (
                    <div className="w-full h-full flex items-center justify-center text-muted-foreground">
                      No Preview
                    </div>
                  )}
                  <div className="absolute top-2 right-2 bg-black/70 text-white text-xs px-2 py-1 rounded-full uppercase font-bold tracking-wider">
                    {resource.category}
                  </div>
                </div>
                <div className="p-4">
                  <h3 className="font-bold text-lg leading-tight mb-2 line-clamp-2 group-hover:text-primary transition-colors">
                    {resource.title}
                  </h3>
                  <p className="text-sm text-muted-foreground">
                    {new Date(resource.created_at).toLocaleDateString()}
                  </p>
                </div>
              </div>
            ))}
          </div>
        )}
      </main>

      {selectedResource && (
        <ResourceDetail
          resource={selectedResource}
          onClose={() => setSelectedResource(null)}
        />
      )}
    </div>
  );
}

export default App;
