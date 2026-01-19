import * as React from "react"
import { cn } from "../../lib/utils"

const Switch = React.forwardRef<
    HTMLInputElement,
    Omit<React.InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange'> & {
        checked: boolean;
        onCheckedChange: (checked: boolean) => void;
    }
>(({ className, checked, onCheckedChange, ...props }, ref) => (
    <label className={cn("inline-flex items-center cursor-pointer", className)}>
        <input
            type="checkbox"
            className="sr-only peer"
            ref={ref}
            checked={checked}
            onChange={(e) => onCheckedChange(e.target.checked)}
            {...props}
        />
        <div className="relative w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-ring rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-primary"></div>
    </label>
))
Switch.displayName = "Switch"

export { Switch }
