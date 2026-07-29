// Interaction proof, not a render-only check (rules-library/core/73-verification.md
// §"Behaviour vs Appearance"): asserts the migrated @poodle64/ui components
// genuinely respond to a click, rather than only rendering. Covers the two
// shapes WP-125's migration touched most — a simple bindable toggle
// (Checkbox) and a portal-based compound component (Dialog).
import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import SharedUiSmoke from './shared-ui-smoke.svelte';

describe('@poodle64/ui consumption (WP-125 migration)', () => {
  it('toggles the migrated Checkbox on click', async () => {
    render(SharedUiSmoke);

    const checkbox = screen.getByRole('checkbox', { name: 'smoke-test-checkbox' });
    expect(checkbox).toHaveAttribute('aria-checked', 'false');

    await fireEvent.click(checkbox);

    await waitFor(() => {
      expect(checkbox).toHaveAttribute('aria-checked', 'true');
    });
  });

  it('opens the migrated Dialog on trigger click and shows its content', async () => {
    render(SharedUiSmoke);

    expect(screen.queryByText('Smoke test dialog')).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));

    await waitFor(() => {
      expect(screen.getByText('Smoke test dialog')).toBeInTheDocument();
    });
  });
});
