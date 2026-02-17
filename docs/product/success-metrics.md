# Success Metrics

How we measure whether Thoth is achieving its product intent.

## Primary Success Metrics

### 1. Time to text

**What**: Elapsed time from hotkey press to transcribed text appearing at cursor.

**Target**: Near-instant. The faster the better - speed is the core value proposition.

### 2. Transcription accuracy

**What**: Percentage of transcribed words that match spoken intent.

**Target**: > 95% word accuracy for clear English speech.

**Measurement**: Qualitative. The user's willingness to keep using voice input is the signal.

### 3. Daily adoption

**What**: The user chooses voice input over typing for suitable content.

**Target**: Multiple dictation sessions per day on active work days.

### 4. Zero-configuration sessions

**What**: Percentage of dictation sessions that require no user intervention beyond pressing the hotkey.

**Target**: > 99% of sessions complete without opening settings or troubleshooting.

## Secondary Metrics

### 5. App launch to ready

**What**: Time from app launch to ready for dictation.

**Target**: < 3 seconds on Apple Silicon.

### 6. Memory footprint

**What**: Resident memory during idle and active transcription.

**Target**: < 500MB during transcription.

### 7. Reliability

**What**: Transcription completes successfully without crashes or hangs.

**Target**: Zero crashes per week of normal use.

## Success Criteria Summary

### MVP Success

- Dictation works reliably with one hotkey
- Text appears at cursor near-instantly after speech ends
- App runs stably for a full work day without intervention
- Setup completes in under 2 minutes

### Long-term Success

- User forgets Thoth is a separate application
- Voice input becomes the default for suitable text entry
- The tool requires no maintenance between macOS updates

## Failure Indicators

- **User opens settings during dictation**: Something is wrong.
- **User switches back to typing**: Transcription is too slow or unreliable.
- **User disables Thoth at login**: The app is causing problems without value.
