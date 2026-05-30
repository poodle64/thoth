import { z } from 'zod';

export const dictionarySchema = z.object({
  from: z.string().min(1, 'Required'),
  to: z.string().min(1, 'Required'),
  caseSensitive: z.boolean().default(false),
});

export type DictionaryFormData = z.infer<typeof dictionarySchema>;
