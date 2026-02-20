/**
 * Toast notification store using Svelte 5 runes.
 *
 * Provides a centralised way for any component to show toast notifications.
 * Toasts are rendered by the <Toaster /> component in the root layout,
 * ensuring they're never clipped by scroll containers.
 */

export type ToastType = 'success' | 'error' | 'info';

export interface Toast {
  id: number;
  type: ToastType;
  message: string;
  dismissible: boolean;
}

interface ToastOptions {
  /** Auto-dismiss after this many milliseconds. 0 = no auto-dismiss. Default: 3000 */
  duration?: number;
  /** Whether the user can manually dismiss. Default: true */
  dismissible?: boolean;
}

const DEFAULT_DURATION = 3000;

function createToastStore() {
  let toasts = $state<Toast[]>([]);
  let nextId = 0;
  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  function push(type: ToastType, message: string, options: ToastOptions = {}): number {
    const id = nextId++;
    const { duration = DEFAULT_DURATION, dismissible = true } = options;

    toasts = [...toasts, { id, type, message, dismissible }];

    if (duration > 0) {
      const timer = setTimeout(() => {
        dismiss(id);
      }, duration);
      timers.set(id, timer);
    }

    return id;
  }

  function dismiss(id: number): void {
    const timer = timers.get(id);
    if (timer) {
      clearTimeout(timer);
      timers.delete(id);
    }
    toasts = toasts.filter((t) => t.id !== id);
  }

  function success(message: string, options?: ToastOptions): number {
    return push('success', message, options);
  }

  function error(message: string, options?: ToastOptions): number {
    return push('error', message, { duration: 0, dismissible: true, ...options });
  }

  function info(message: string, options?: ToastOptions): number {
    return push('info', message, options);
  }

  return {
    get toasts() {
      return toasts;
    },
    push,
    dismiss,
    success,
    error,
    info,
  };
}

export const toastStore = createToastStore();
