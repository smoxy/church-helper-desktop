import { createBrowserRouter, RouterProvider } from "react-router-dom";
import AppLayout from "./layouts/AppLayout";
import Dashboard from "./pages/Dashboard";
import Settings from "./pages/Settings";
import { SplashScreen } from "./components/ui/SplashScreen";

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
  return (
    <>
      <RouterProvider router={router} />
      <SplashScreen />
    </>
  );
}

export default App;
