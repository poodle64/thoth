---
paths: '**/*'
---

# Troubleshooting and Debugging

## Core Principle

When debugging issues, **always examine the actual evidence** (logs, console output, error messages) before adding more code or making assumptions.

## Process

### 1. Gather Evidence First

**Before writing any code:**

- Check terminal/console logs for actual error messages
- Look for warnings or failures in the output
- Verify what's actually happening vs what's expected
- Use existing debug logging to understand the flow

**How to check logs in this project:**

- Terminal running `pnpm tauri dev` shows Rust backend logs
- Browser DevTools console shows frontend logs
- Frontend indicator window emits logs with `[Indicator]` prefix
- Backend has `#[cfg(debug_assertions)]` conditional logging

### 2. Understand the Problem

- Read error messages completely and carefully
- Trace the execution flow using existing logs
- Identify where the flow breaks down
- Don't assume - verify with evidence

### 3. Only Then Fix

- Make targeted changes based on evidence
- Don't add speculative code
- Don't add more logging without checking existing logs first
- Test the fix and verify with logs

## Anti-Patterns to Avoid

❌ **Don't:**

- Add more logging before checking existing logs
- Make assumptions about what's failing
- Write speculative fixes without understanding the problem
- Add complexity when the issue might be simple
- Keep coding when asked to check logs

✅ **Do:**

- Stop and examine evidence when asked
- Read and interpret actual log output
- Ask user for specific log output when you can't access it directly
- Make minimal, targeted changes based on evidence
- Verify fixes with the same evidence that showed the problem

## Example: Audio Events Not Working

**Wrong approach:**

```
User: "The equalizer isn't working"
Assistant: *adds more code* *adds more logging* *changes event emission*
```

**Right approach:**

```
User: "The equalizer isn't working"
Assistant: "Let me check the logs - can you share what you see when you start recording?"
User: *shares logs showing 'listener not registered' error*
Assistant: "The logs show the listener isn't being registered. The issue is in the onMount timing..."
```

## Logging in This Project

### Backend (Rust)

- Use `tracing::info!()` for important flow milestones
- Use `tracing::debug!()` for detailed debugging (dev builds only)
- Use `tracing::warn!()` for recoverable issues
- Use `tracing::error!()` for failures
- Wrap verbose logging in `#[cfg(debug_assertions)]` for dev-only

### Frontend (Svelte)

- Indicator window uses `indicatorLog()` helper that emits to main window
- Messages are prefixed with `[Indicator]` for easy filtering
- Check browser DevTools console for frontend logs

## When User Says "Check the Logs"

**Stop immediately and:**

1. Ask user to share relevant log output (if you can't access it)
2. Read and interpret what the logs are saying
3. Identify the root cause from the evidence
4. Only then propose a fix

Do **not** continue adding code or features when explicitly asked to examine logs.
