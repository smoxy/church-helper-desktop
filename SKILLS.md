# Church Helper Desktop - UI/UX Manual & Skills

This document outlines the "Golden Rules" of UI/UX design for this project and defines the color palette to ensure a modern, premium, and trustworthy aesthetic.

## Golden Rules of UI/UX

1.  **Clarity & Simplicity**: Interfaces must be straightforward. Eliminate clutter. Users should instantly know what to do.
2.  **Consistency**: Use uniform buttons, icons, and spacing. Re-use components to build familiarity.
3.  **Visual Hierarchy**: Use typography (size, weight) and contrast to guide the user's eye to key actions (e.g., the "Download" button).
4.  **Feedback**: Provide immediate visual feedback for all interactions (hover states, click ripples, loading spinners).
5.  **Alignment & Spacing**: Use a consistent spacing scale (multiples of 4px). Proper whitespace "elevates" the design.
6.  **Accessibility**: Ensure high contrast for text and keyboard navigability.
7.  **Delight**: Add subtle micro-animations (e.g., smooth transitions when opening modals) to feel "alive".

## Project Color Palette

We use a "Trustworthy & Premium" palette based on deep blues and clean whites/grays.

| Role | Color Name | Hex | Tailwind Variable (HSL) | Usage |
| :--- | :--- | :--- | :--- | :--- |
| **Primary** | **Royal Blue** | `#2C5AA0` | `214 57% 40%` | Primary buttons, active states, key headers. |
| **Secondary** | **Soft Sky** | `#4A90E2` | `212 72% 59%` | Accents, icons, secondary actions. |
| **Success** | **Growth Green** | `#27AE60` | `145 63% 42%` | Download completed, success messages. |
| **Warning/Action** | **Vibrant Orange** | `#F39C12` | `37 90% 51%` | "Download Needed" status, attention grabbers. |
| **Background** | **Clean White** | `#F8FAFC` | `210 20% 98%` | Main app background (light mode). |
| **Surface** | **Pure White** | `#FFFFFF` | `0 0% 100%` | Cards, modals, sidebars. |
| **Text Main** | **Dark Navy** | `#1A202C` | `220 26% 14%` | Primary text. |
| **Text Muted** | **Slate Gray** | `#718096` | `215 16% 47%` | Secondary text, labels. |

### Implementation in Code
Use Tailwind CSS classes referencing these variables (e.g., `bg-primary`, `text-primary-foreground`).

## Component Structure Guidelines

To maintain a scalable codebase:
-   **`src/components/common`**: Generic UI elements (Buttons, Inputs, Modals) - *dumb components*.
-   **`src/components/features`**: Domain-specific components (e.g., `ResourceCard`, `ResourceDetail`).
-   **`src/hooks`**: Custom React hooks for logic (e.g., `useResourceDownload`).
-   **`src/stores`**: State management (Zustand/Context).

Always prefer composing small, single-responsibility components over monolithic ones.

## Accessibility Testing

We maintain strict accessibility standards (WCAG 2.1 AA).

### Automated Contrast Testing
To verify color contrast ratios recursively across the application:

1.  Ensure the development server is running: `npm run tauri dev`
2.  In a separate terminal, run: `npm run test:contrast`

This uses `pa11y-ci` to crawl the application and report any contrast violations.
