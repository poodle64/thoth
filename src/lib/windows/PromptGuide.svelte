<script lang="ts">
  /**
   * Prompt Writing Guide Window
   *
   * Displays a user-friendly guide for writing effective custom prompts.
   */
</script>

<div class="guide-window">
  <div class="guide-header">
    <h1>Writing Effective Custom Prompts</h1>
    <p class="subtitle">Create custom AI enhancement prompts that produce consistent, high-quality results</p>
  </div>

  <div class="guide-content">
    <section>
      <h2>Quick Start</h2>
      <p>
        A custom prompt is a template that tells the AI how to transform your transcribed text. The
        template must include <code>{'{text}'}</code> as a placeholder for your transcription.
      </p>
      <div class="example-box">
        <div class="example-label">Basic template structure:</div>
        <pre>[Task instruction]. [Constraints]. [Output directive].

{'{text}'}</pre>
      </div>
    </section>

    <section>
      <h2>Core Principles</h2>

      <h3>1. Be Specific About the Task</h3>
      <p>Instead of vague instructions, clearly define what transformation you want.</p>

      <div class="comparison">
        <div class="bad-example">
          <div class="badge bad">❌ Vague</div>
          <pre>Make this better:

{'{text}'}</pre>
        </div>
        <div class="good-example">
          <div class="badge good">✅ Specific</div>
          <pre>Rewrite this as a professional email. Use formal greeting and closing. Keep the main points intact.

{'{text}'}</pre>
        </div>
      </div>

      <h3>2. Set Clear Constraints</h3>
      <p>Prevent the AI from over-elaborating by specifying length, style, and content boundaries.</p>

      <div class="info-box">
        <strong>Essential constraints to include:</strong>
        <ul>
          <li><strong>Length:</strong> "Keep the same approximate length" or "Limit to 2-3 sentences"</li>
          <li><strong>Scope:</strong> "Do not add extra content or explanations"</li>
          <li><strong>Tone:</strong> "Maintain professional/casual/technical tone"</li>
          <li><strong>Format:</strong> "Output as bullet points" or "Write as single paragraph"</li>
        </ul>
      </div>

      <div class="comparison">
        <div class="bad-example">
          <div class="badge bad">❌ No constraints</div>
          <pre>Rewrite this in pirate speak:

{'{text}'}</pre>
          <p class="problem">Problem: May produce 5 paragraphs from 2 sentences</p>
        </div>
        <div class="good-example">
          <div class="badge good">✅ With constraints</div>
          <pre>Rewrite this in pirate dialect. Keep the same approximate length and meaning. Do not add extra content or explanations. Only output the rewritten text:

{'{text}'}</pre>
        </div>
      </div>

      <h3>3. Direct the Output Format</h3>
      <p>Always end with a clear output directive to prevent the AI from adding commentary.</p>

      <div class="info-box">
        <strong>Recommended output directives:</strong>
        <ul>
          <li>"Only output the [rewritten/corrected/summarised] text:"</li>
          <li>"Respond with only the transformed text, nothing else:"</li>
          <li>"Output the result directly without any preamble or explanation:"</li>
        </ul>
      </div>
    </section>

    <section>
      <h2>Template Patterns</h2>

      <h3>Length-Preserving Transformations</h3>
      <p>For prompts that should maintain similar length (grammar fixes, tone changes, translations):</p>
      <div class="example-box">
        <pre>[Action verb] the following text to [goal]. Keep the same meaning and approximate length. Do not add extra content or explanations. Only output the [transformed] text:

{'{text}'}</pre>
        <p class="examples">
          <strong>Examples:</strong> Fix grammar mistakes, Translate to Spanish, Rewrite in technical language
        </p>
      </div>

      <h3>Length-Reducing Transformations</h3>
      <p>For prompts that should condense text (summaries, key points):</p>
      <div class="example-box">
        <pre>[Action verb] the following text to [goal]. Limit to [specific constraint]. Keep only the most important [elements]. Only output the [result]:

{'{text}'}</pre>
        <p class="examples">
          <strong>Examples:</strong> Summarise in 1-2 sentences, Extract 3-5 key action items, Reduce to main conclusion
        </p>
      </div>

      <h3>Length-Expanding Transformations</h3>
      <p>For prompts that should add detail (expansions, elaborations):</p>
      <div class="example-box">
        <pre>[Action verb] the following text with [specific amount] more [element]. Keep the same [aspects]. Only output the expanded text:

{'{text}'}</pre>
        <p class="examples">
          <strong>Examples:</strong> Expand with 2-3x more detail, Elaborate with specific examples, Add technical context
        </p>
      </div>
    </section>

    <section>
      <h2>Example Custom Prompts</h2>

      <div class="example-prompt">
        <h4>Meeting Notes Formatter</h4>
        <pre>Transform the following transcription into structured meeting notes. Use these sections: Summary, Key Points, Action Items, Next Steps. Use bullet points for lists. Only output the formatted notes:

{'{text}'}</pre>
      </div>

      <div class="example-prompt">
        <h4>Technical Documentation Style</h4>
        <pre>Rewrite the following text in technical documentation style. Use clear, precise language. Define any acronyms on first use. Keep the same approximate length. Only output the rewritten text:

{'{text}'}</pre>
      </div>

      <div class="example-prompt">
        <h4>Email Draft Generator</h4>
        <pre>Transform the following notes into a professional email draft. Include appropriate greeting and closing. Organise into clear paragraphs. Keep a friendly but professional tone. Only output the email:

{'{text}'}</pre>
      </div>

      <div class="example-prompt">
        <h4>Action Item Extractor</h4>
        <pre>Extract concrete action items from the following text. Format as bullet points with clear verb-noun structure. Include only actionable tasks. Only output the action items:

{'{text}'}</pre>
      </div>
    </section>

    <section>
      <h2>Model Considerations</h2>

      <div class="model-info">
        <div class="model-category">
          <h4>Larger models (7B+)</h4>
          <ul>
            <li>Better at following complex instructions</li>
            <li>May over-elaborate without strong constraints</li>
            <li>Can handle multi-step transformations</li>
          </ul>
        </div>

        <div class="model-category">
          <h4>Smaller models (1.5B-3B)</h4>
          <ul>
            <li>Need simpler, more direct instructions</li>
            <li>More sensitive to prompt length</li>
            <li>Work best with single-focus transformations</li>
          </ul>
        </div>
      </div>

      <div class="tip-box">
        <strong>💡 Tip:</strong> If using a small model (like Qwen 2.5 1.5B/3B), keep prompts focused and constraints explicit. Test your prompts with your specific model and adjust based on results.
      </div>
    </section>

    <section>
      <h2>Troubleshooting</h2>

      <table class="troubleshooting-table">
        <thead>
          <tr>
            <th>Problem</th>
            <th>Solution</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Output is too long</td>
            <td>Add "Keep the same approximate length" and "Do not add extra content"</td>
          </tr>
          <tr>
            <td>Output includes preamble</td>
            <td>Add "Only output the [result]:" at end</td>
          </tr>
          <tr>
            <td>Output misses the point</td>
            <td>Make task more specific and concrete</td>
          </tr>
          <tr>
            <td>Output is too creative</td>
            <td>Add "Stay close to the original meaning"</td>
          </tr>
          <tr>
            <td>Inconsistent results</td>
            <td>Add more specific task definition and constraints</td>
          </tr>
        </tbody>
      </table>
    </section>

    <section>
      <h2>Template Checklist</h2>
      <div class="checklist">
        <p>Before saving a custom prompt, verify:</p>
        <ul>
          <li>✓ Contains <code>{'{text}'}</code> placeholder</li>
          <li>✓ Has clear task definition</li>
          <li>✓ Specifies desired output length/scope</li>
          <li>✓ Includes content constraints ("do not add...")</li>
          <li>✓ Ends with output directive ("only output...")</li>
          <li>✓ Tested with sample input</li>
          <li>✓ Produces consistent results</li>
        </ul>
      </div>
    </section>
  </div>
</div>

<style>
  .guide-window {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--color-bg-primary);
    color: var(--color-text-primary);
    overflow: hidden;
  }

  .guide-header {
    padding: 24px 32px;
    background: var(--color-bg-secondary);
    border-bottom: 1px solid var(--color-border);
  }

  .guide-header h1 {
    margin: 0 0 8px 0;
    font-size: 24px;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .subtitle {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .guide-content {
    flex: 1;
    overflow-y: auto;
    padding: 32px;
    max-width: 900px;
    margin: 0 auto;
    width: 100%;
  }

  section {
    margin-bottom: 48px;
  }

  section:last-child {
    margin-bottom: 0;
  }

  h2 {
    margin: 0 0 16px 0;
    font-size: 20px;
    font-weight: 600;
    color: var(--color-text-primary);
    border-bottom: 2px solid var(--color-accent);
    padding-bottom: 8px;
  }

  h3 {
    margin: 24px 0 12px 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  h4 {
    margin: 0 0 8px 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-accent);
  }

  p {
    margin: 0 0 16px 0;
    line-height: 1.6;
    color: var(--color-text-secondary);
  }

  code {
    padding: 2px 6px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.9em;
    color: var(--color-accent);
  }

  pre {
    margin: 0;
    padding: 12px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-md);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.5;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .example-box {
    margin: 16px 0;
    padding: 16px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
  }

  .example-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }

  .examples {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--color-border-subtle);
    font-size: var(--text-sm);
    color: var(--color-text-tertiary);
  }

  .comparison {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    margin: 16px 0;
  }

  @media (max-width: 768px) {
    .comparison {
      grid-template-columns: 1fr;
    }
  }

  .bad-example,
  .good-example {
    padding: 16px;
    border-radius: var(--radius-md);
    border: 2px solid;
  }

  .bad-example {
    background: color-mix(in srgb, var(--color-error) 5%, transparent);
    border-color: color-mix(in srgb, var(--color-error) 30%, transparent);
  }

  .good-example {
    background: color-mix(in srgb, var(--color-success) 5%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 30%, transparent);
  }

  .badge {
    display: inline-block;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    margin-bottom: 8px;
  }

  .badge.bad {
    background: var(--color-error);
    color: white;
  }

  .badge.good {
    background: var(--color-success);
    color: white;
  }

  .problem {
    margin-top: 8px;
    font-size: var(--text-xs);
    color: var(--color-error);
    font-style: italic;
  }

  .info-box {
    margin: 16px 0;
    padding: 16px;
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    border-left: 4px solid var(--color-accent);
    border-radius: var(--radius-md);
  }

  .info-box ul {
    margin: 8px 0 0 0;
    padding-left: 24px;
  }

  .info-box li {
    margin: 6px 0;
    color: var(--color-text-secondary);
  }

  .tip-box {
    margin: 16px 0;
    padding: 16px;
    background: color-mix(in srgb, var(--color-accent) 5%, transparent);
    border-radius: var(--radius-md);
    border: 1px solid color-mix(in srgb, var(--color-accent) 20%, transparent);
  }

  .example-prompt {
    margin: 16px 0;
    padding: 16px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
  }

  .model-info {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    margin: 16px 0;
  }

  @media (max-width: 768px) {
    .model-info {
      grid-template-columns: 1fr;
    }
  }

  .model-category {
    padding: 16px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
  }

  .model-category h4 {
    margin-top: 0;
  }

  .model-category ul {
    margin: 0;
    padding-left: 24px;
  }

  .model-category li {
    margin: 6px 0;
    color: var(--color-text-secondary);
  }

  .troubleshooting-table {
    width: 100%;
    border-collapse: collapse;
    margin: 16px 0;
  }

  .troubleshooting-table th,
  .troubleshooting-table td {
    padding: 12px;
    text-align: left;
    border: 1px solid var(--color-border);
  }

  .troubleshooting-table th {
    background: var(--color-bg-secondary);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .troubleshooting-table td {
    background: var(--color-bg-primary);
    color: var(--color-text-secondary);
  }

  .checklist {
    padding: 16px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
  }

  .checklist p {
    margin-bottom: 12px;
    font-weight: 500;
  }

  .checklist ul {
    margin: 0;
    padding-left: 24px;
    list-style: none;
  }

  .checklist li {
    margin: 8px 0;
    color: var(--color-text-secondary);
  }

  .checklist li::before {
    content: '✓ ';
    color: var(--color-success);
    font-weight: bold;
    margin-right: 8px;
  }
</style>
