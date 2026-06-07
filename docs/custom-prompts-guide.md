# Writing effective custom prompts

This guide helps you create custom AI enhancement prompts that produce consistent, high-quality results.

## Before you start

AI enhancement is optional and stays off until you enable it. It runs your transcription through a local large language model to tidy grammar, adjust tone, or reformat; nothing leaves your machine unless you deliberately point it at a remote endpoint.

You need a model endpoint:

- **[Ollama](https://ollama.com)** is the default. Install it, pull a model (for example `ollama pull llama3.2`), and make sure it is running.
- Any **OpenAI-compatible endpoint** also works (LM Studio, llama.cpp's server, vLLM, and similar), which is handy if you already run one.

Then open **Settings > AI Enhancement** in Thoth, turn it on, and confirm the endpoint and model. Once it is enabled you can pick a built-in prompt or write your own using the rest of this guide.

## Quick start

A custom prompt is a template that tells the AI how to transform your transcribed text. The template must include `{text}` as a placeholder for your transcription.

**Basic template structure:**

```text
[Task instruction]. [Constraints]. [Output directive].

{text}
```

## Core principles

### 1. Be specific about the task

Instead of vague instructions, clearly define what transformation you want.

❌ **Vague:**

```text
Make this better:

{text}
```

✅ **Specific:**

```text
Rewrite this as a professional email. Use formal greeting and closing. Keep the main points intact.

{text}
```

### 2. Set clear constraints

Prevent the AI from over-elaborating by specifying length, style, and content boundaries.

**Essential constraints to include:**

- **Length:** "Keep the same approximate length" or "Limit to 2-3 sentences"
- **Scope:** "Do not add extra content or explanations"
- **Tone:** "Maintain professional/casual/technical tone"
- **Format:** "Output as bullet points" or "Write as single paragraph"

❌ **No constraints:**

```text
Rewrite this in pirate speak:

{text}
```

_Problem: May produce 5 paragraphs from 2 sentences_

✅ **With constraints:**

```text
Rewrite this in pirate dialect. Keep the same approximate length and meaning. Do not add extra content or explanations. Only output the rewritten text:

{text}
```

### 3. Direct the output format

Always end with a clear output directive to prevent the AI from adding commentary.

**Recommended output directives:**

- "Only output the [rewritten/corrected/summarised] text:"
- "Respond with only the transformed text, nothing else:"
- "Output the result directly without any preamble or explanation:"

## Template patterns

### Length-preserving transformations

For prompts that should maintain similar length (grammar fixes, tone changes, translations):

```text
[Action verb] the following text to [goal]. Keep the same meaning and approximate length. Do not add extra content or explanations. Only output the [transformed] text:

{text}
```

**Examples:**

- "Fix grammar mistakes in the following text..."
- "Translate the following text to Spanish..."
- "Rewrite the following text in technical language..."

### Length-reducing transformations

For prompts that should condense text (summaries, key points):

```text
[Action verb] the following text to [goal]. Limit to [specific constraint]. Keep only the most important [elements]. Only output the [result]:

{text}
```

**Examples:**

- "Summarise the following text concisely in 1-2 sentences..."
- "Extract the 3-5 key action items from the following text..."
- "Reduce the following text to its main conclusion..."

### Length-expanding transformations

For prompts that should add detail (expansions, elaborations):

```text
[Action verb] the following text with [specific amount] more [element]. Keep the same [aspects]. Only output the expanded text:

{text}
```

**Examples:**

- "Expand the following text with 2-3x more detail and explanation..."
- "Elaborate on the following text with specific examples..."
- "Add technical context and background to the following text..."

### Format transformations

For prompts that change structure (bullet points, code, formal documents):

```text
Transform the following text into [format]. [Structure requirements]. [Content requirements]. Only output the formatted text:

{text}
```

**Examples:**

- "Transform the following text into bullet points. Use concise phrases. Group related items..."
- "Convert the following text into a formal meeting agenda with time allocations..."
- "Rewrite the following text as a code comment with proper formatting..."

## Common pitfalls

### Over-elaboration

**Problem:** AI adds unwanted content

```text
Improve this text:

{text}
```

**Solution:** Add explicit constraints

```text
Fix grammar and improve clarity in the following text. Keep the same meaning and approximate length. Do not add extra content or explanations. Only output the corrected text:

{text}
```

### Unwanted preambles

**Problem:** AI adds "Here is the rewritten text:" before output

```text
Rewrite this professionally.

{text}
```

**Solution:** Add explicit output directive

```text
Rewrite this professionally. Keep the same meaning. Only output the rewritten text:

{text}
```

### Inconsistent results

**Problem:** AI interprets vague instructions differently each time

```text
Make this sound better:

{text}
```

**Solution:** Define specific transformation goals

```text
Rewrite the following text to be more confident and assertive. Use active voice and strong verbs. Keep the same meaning and approximate length. Only output the rewritten text:

{text}
```

### Missing placeholder

**Problem:** Prompt doesn't include `{text}`

```text
Fix any grammar mistakes in the transcription.
```

**Solution:** Always include `{text}` placeholder

```text
Fix any grammar mistakes in the following text. Only output the corrected text:

{text}
```

## Testing your prompts

1. **Test with short input** (1-2 sentences)
   - Verify it doesn't over-elaborate
   - Check output format is clean

2. **Test with medium input** (3-5 sentences)
   - Verify length constraints work
   - Check tone/style is consistent

3. **Test with long input** (paragraph+)
   - Verify it maintains focus
   - Check it doesn't summarise unintentionally

4. **Test edge cases**
   - Very short input (3-5 words)
   - Technical jargon
   - Proper nouns and names

## Example custom prompts

### Meeting notes formatter

```text
Transform the following transcription into structured meeting notes. Use these sections: Summary, Key Points, Action Items, Next Steps. Use bullet points for lists. Only output the formatted notes:

{text}
```

### Technical documentation style

```text
Rewrite the following text in technical documentation style. Use clear, precise language. Define any acronyms on first use. Keep the same approximate length. Only output the rewritten text:

{text}
```

### Email draft generator

```text
Transform the following notes into a professional email draft. Include appropriate greeting and closing. Organise into clear paragraphs. Keep a friendly but professional tone. Only output the email:

{text}
```

### Code comment generator

```text
Transform the following explanation into properly formatted code comments. Use clear, concise language. Follow standard comment conventions. Only output the comments:

{text}
```

### Action item extractor

```text
Extract concrete action items from the following text. Format as bullet points with clear verb-noun structure. Include only actionable tasks. Only output the action items:

{text}
```

## Model considerations

Different models respond differently to prompts:

**Larger models (7B+):**

- Better at following complex instructions
- May over-elaborate without strong constraints
- Can handle multi-step transformations

**Smaller models (1.5B-3B):**

- Need simpler, more direct instructions
- More sensitive to prompt length
- Work best with single-focus transformations

**Recommendations:**

- If using a small model (like Qwen 2.5 1.5B/3B), keep prompts focused and constraints explicit
- Test your prompts with your specific model
- Adjust constraint strength based on results

## Troubleshooting

| Problem                  | Solution                                                              |
| ------------------------ | --------------------------------------------------------------------- |
| Output is too long       | Add "Keep the same approximate length" and "Do not add extra content" |
| Output includes preamble | Add "Only output the [result]:" at end                                |
| Output misses the point  | Make task more specific and concrete                                  |
| Output is too creative   | Add "Stay close to the original meaning"                              |
| Output is too literal    | Remove overly strict constraints                                      |
| Inconsistent results     | Add more specific task definition and constraints                     |

## Template checklist

Before saving a custom prompt, verify:

- [ ] Contains `{text}` placeholder
- [ ] Has clear task definition
- [ ] Specifies desired output length/scope
- [ ] Includes content constraints ("do not add...")
- [ ] Ends with output directive ("only output...")
- [ ] Tested with sample input
- [ ] Produces consistent results

## Getting help

If your custom prompt isn't working as expected:

1. Start with a built-in prompt as a template
2. Add one constraint at a time
3. Test after each change
4. Compare with similar built-in prompts
5. Try simplifying the instruction

The built-in prompts in Thoth follow these best practices and can serve as templates for your custom prompts.
