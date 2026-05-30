import { z } from 'zod';

export const promptSchema = z.object({
  name: z.string().min(1, 'Name required'),
  template: z
    .string()
    .min(1, 'Template required')
    .refine((t) => t.includes('{text}'), { message: 'Template must contain {text}' }),
});

export type PromptFormData = z.infer<typeof promptSchema>;
