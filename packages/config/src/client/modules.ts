import { z } from 'zod'

/** Planning metadata only. Permissions here never grant runtime authority. */
export const moduleManifestSchema = z
  .object({
    id: z.string().regex(/^[a-z][a-z0-9_-]*$/),
    version: z.string().regex(/^\d+\.\d+\.\d+$/),
    status: z.enum(['implemented', 'planned']),
    requires: z.array(z.string().min(1)).default([]),
    permissions: z.array(z.string().min(1)).default([]),
    apiVersion: z
      .string()
      .regex(/^\d+\.\d+\.\d+$/)
      .optional(),
    persistence: z
      .object({ adapter: z.string().min(1), migration: z.string().min(1) })
      .strict()
      .optional(),
    tools: z.array(z.string().min(1)).optional(),
  })
  .strict()

export type ModuleManifest = z.infer<typeof moduleManifestSchema>

/** The template vocabulary includes future capabilities, not installed products. */
export const PLATFORM_MODULES: readonly ModuleManifest[] = [
  {
    id: 'knowledge',
    version: '0.1.0',
    status: 'implemented',
    requires: [],
    permissions: ['knowledge.read'],
  },
  {
    id: 'page_awareness',
    version: '0.1.0',
    status: 'implemented',
    requires: [],
    permissions: ['page.read'],
  },
  {
    id: 'handoff',
    version: '0.1.0',
    status: 'implemented',
    requires: [],
    permissions: ['handoff.prepare'],
  },
  ...[
    'analytics',
    'appointment',
    'availability',
    'booking',
    'cart',
    'checkout',
    'comparison',
    'crm',
    'forms',
    'lead_capture',
    'lead_qualification',
    'menu_recommendation',
    'order_lookup',
    'payment',
    'post_purchase',
    'product_recommendation',
    'program_matching',
    'property_recommendation',
    'receipt',
    'recommendations',
    'reservation',
    'service_matching',
    'transactional_email',
    'viewing_booking',
  ].map(
    (id): ModuleManifest => ({
      id,
      version: '0.1.0',
      status: 'planned',
      requires: [],
      permissions: [],
    }),
  ),
]

/** Host-owned registry. A tenant config cannot register its own implementation. */
export class ModuleRegistry {
  private readonly manifests = new Map<string, ModuleManifest>()

  constructor(manifests: readonly ModuleManifest[] = PLATFORM_MODULES) {
    for (const raw of manifests) {
      const manifest = moduleManifestSchema.parse(raw)
      if (this.manifests.has(manifest.id)) throw new Error(`Duplicate module: ${manifest.id}`)
      this.manifests.set(manifest.id, manifest)
    }
    for (const manifest of this.manifests.values()) {
      for (const id of manifest.requires) {
        if (!this.manifests.has(id))
          throw new Error(`Module ${manifest.id} requires unknown module ${id}`)
      }
    }
    // Validate the whole registry, even dependencies not selected by this client.
    const visited = new Set<string>()
    const visiting = new Set<string>()
    const visit = (id: string): void => {
      if (visiting.has(id)) throw new Error(`Circular module dependency: ${id}`)
      if (visited.has(id)) return
      visiting.add(id)
      for (const dependency of this.get(id).requires) visit(dependency)
      visiting.delete(id)
      visited.add(id)
    }
    for (const id of this.manifests.keys()) visit(id)
  }

  get(id: string): ModuleManifest {
    const manifest = this.manifests.get(id)
    if (!manifest) throw new Error(`Unknown module: ${id}; register an implementation explicitly`)
    return { ...manifest, requires: [...manifest.requires], permissions: [...manifest.permissions] }
  }
}
