import { useEffect } from "react";
import { createBrowserRouter, RouterProvider } from "react-router-dom";
import AppLayout from "./layouts/AppLayout";
import Dashboard from "./pages/Dashboard";
import Settings from "./pages/Settings";
import { SplashScreen } from "./components/ui/SplashScreen";
import { useAppStore } from "./stores/appStore";
import { applyTheme } from "./lib/theme";

const router = createBrowserRouter([
  {
    path: "/",
    element: <AppLayout />,
    children: [
      {
        path: "/",
        element: <Dashboard />,
      },
      {
        path: "/settings",
        element: <Settings />,
      },
    ],
  },
]);

/**
 * Application root, mounted by main.tsx. Renders the router plus the
 * startup splash overlay on top of it (see SplashScreen for its own
 * self-contained visibility timer — it appears on every launch).
 */
function App() {
  const theme = useAppStore(s => s.config?.theme);
  useEffect(() => {
    // Config not loaded yet: leave initThemeFromCache's pre-paint hint
    // (see main.tsx) in place instead of forcing 'System', which would
    // flash a saved Dark theme to light on an OS-light machine.
    if (theme === undefined) return;
    return applyTheme(theme);
  }, [theme]);

  return (
    <>
      <RouterProvider router={router} />
      <SplashScreen />
    </>
  );
}

export default App;
