import { useEffect } from "react";
import { useNavigate } from "react-router-dom";

/**
 * App.tsx is currently not used directly as the main UI entry point 
 * because the application uses react-router-dom in main.tsx.
 * 
 * We keep it as a simple redirector or placeholder.
 */
function App() {
  const navigate = useNavigate();

  useEffect(() => {
    // Redirect to the dashboard which is our main entry point
    navigate("/");
  }, [navigate]);

  return null;
}

export default App;
