import React from 'react';
import { useToastStore, ToastType } from '../../stores/toastStore';
import { CheckCircle, AlertCircle, Info, X } from 'lucide-react';
import { cn } from '../../lib/utils';

export const ToastContainer: React.FC = () => {
    const { toasts } = useToastStore();

    return (
        <div className="fixed bottom-6 right-6 z-50 flex flex-col gap-3 pointer-events-none">
            {toasts.map((toast) => (
                <ToastItem key={toast.id} toast={toast} />
            ))}
        </div>
    );
};

interface ToastItemProps {
    toast: {
        id: string;
        message: string;
        type: ToastType;
    };
}

const ToastItem: React.FC<ToastItemProps> = ({ toast }) => {
    const { removeToast } = useToastStore();

    const icons = {
        success: <CheckCircle className="h-5 w-5 text-white" />,
        error: <AlertCircle className="h-5 w-5 text-white" />,
        info: <Info className="h-5 w-5 text-white" />,
    };

    const styles = {
        success: "bg-emerald-600/60 border-emerald-500/50 text-white shadow-emerald-500/20",
        error: "bg-rose-600/80 border-rose-500/50 text-white shadow-rose-500/20",
        info: "bg-blue-600/80 border-blue-500/50 text-white shadow-blue-500/20",
    };

    return (
        <div
            className={cn(
                "pointer-events-auto flex items-center justify-between gap-4 px-4 py-3 rounded-lg border shadow-xl backdrop-blur-md animate-in slide-in-from-right-full fade-in duration-300",
                styles[toast.type]
            )}
        >
            <div className="flex items-center gap-3">
                {icons[toast.type]}
                <p className="text-sm font-bold tracking-wide">{toast.message}</p>
            </div>
            <button
                onClick={() => removeToast(toast.id)}
                className="hover:bg-white/20 p-1 rounded-full transition-colors"
                aria-label="Close"
            >
                <X className="h-4 w-4 text-white" />
            </button>
        </div>
    );
};
