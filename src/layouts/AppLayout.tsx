import { NavLink, Outlet } from "react-router-dom";
import { LayoutDashboard, Settings } from "lucide-react";
import { cn } from "../lib/utils";
import { ToastContainer } from "../components/ui/toast";
import { CelebrationStack } from "../components/ui/CelebrationStack";
import { RinoovaLogo } from "../components/ui/RinoovaLogo";

export default function AppLayout() {
    return (
        <div className="flex h-screen bg-background text-foreground overflow-hidden">
            {/* Sidebar */}
            <aside className="w-64 border-r bg-card flex flex-col">
                <div className="p-6 border-b">
                    <h1 className="text-xl font-bold flex items-center gap-2">
                        <span className="text-primary">Church</span>Helper
                    </h1>
                </div>

                <nav className="flex-1 p-4 space-y-2">
                    <NavLink
                        to="/"
                        className={({ isActive }) =>
                            cn(
                                "flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors",
                                isActive
                                    ? "bg-primary text-primary-foreground"
                                    : "text-muted-foreground hover:bg-muted hover:text-foreground"
                            )
                        }
                    >
                        <LayoutDashboard className="h-4 w-4" />
                        Dashboard
                    </NavLink>

                    <NavLink
                        to="/settings"
                        className={({ isActive }) =>
                            cn(
                                "flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors",
                                isActive
                                    ? "bg-primary text-primary-foreground"
                                    : "text-muted-foreground hover:bg-muted hover:text-foreground"
                            )
                        }
                    >
                        <Settings className="h-4 w-4" />
                        Settings
                    </NavLink>
                </nav>
            </aside>

            {/* Main Content */}
            <main className="flex-1 overflow-auto bg-muted/20 flex flex-col">
                <div className="flex-1 p-8 max-w-6xl mx-auto w-full space-y-8">
                    <Outlet />
                </div>
                <footer className="p-8 max-w-6xl mx-auto w-full border-t text-sm text-muted-foreground">
                    <div className="flex flex-col md:flex-row justify-between items-center gap-4">
                        <div className="space-y-1 text-center md:text-left">
                            <p className="flex items-center justify-center md:justify-start gap-1">
                                <span>© 2026 ChurchHelper</span>
                                <span className="opacity-50">|</span>
                                <span>Open Source under MIT License</span>
                            </p>
                            <p className="text-xs opacity-70">
                                Built with heart for the community.
                            </p>
                        </div>

                        <div className="flex flex-col items-center md:items-end gap-2">
                            <a
                                href="mailto:dev@adventistyouth.it"
                                className="hover:text-primary transition-colors flex items-center gap-1.5"
                            >
                                <span className="text-xs">Support:</span>
                                <span className="font-medium">dev@adventistyouth.it</span>
                            </a>
                            <div className="flex flex-wrap items-center justify-center gap-3">
                                <a
                                    href="https://buymeacoffee.com/smoxy"
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="inline-flex items-center gap-2 px-4 py-2 rounded-full bg-orange-500/10 text-orange-600 hover:bg-orange-500/20 transition-all font-semibold text-xs border border-orange-500/20"
                                >
                                    ☕ Help me paying the server
                                </a>
                                <a
                                    href="https://rinoova.com"
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="inline-flex items-center gap-2 px-4 py-2 rounded-full bg-card text-foreground hover:bg-muted hover:text-primary transition-all font-semibold text-xs border border-border"
                                >
                                    <RinoovaLogo className="h-4" />
                                    Sponsored by Rinoova
                                </a>
                            </div>
                        </div>
                    </div>
                </footer>
            </main>
            <ToastContainer />
            <CelebrationStack />
        </div>
    );
}
