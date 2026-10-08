import { readFileSync } from 'node:fs'
import path from 'node:path'
import { templatesSchema, type Template, type TemplateCatalog } from './schema.js'

export class TemplateError extends Error {
  constructor(
    message: string,
    readonly detail?: unknown,
  ) {
    super(message)
    this.name = 'TemplateError'
  }
}

const TEMPLATES_RELATIVE_PATH = 'config/templates.v1.json'

export function loadTemplates(searchFrom: string = process.cwd()): TemplateCatalog {
  const candidates: string[] = []
  let directory = path.resolve(searchFrom)
  while (true) {
    candidates.push(path.join(directory, TEMPLATES_RELATIVE_PATH))
    const parent = path.dirname(directory)
    if (parent === directory) break
    directory = parent
  }
  const found = candidates.find((candidate) => {
    try {
      readFileSync(candidate, 'utf8')
      return true
    } catch {
      return false
    }
  })
  if (!found) {
    throw new TemplateError(
      `Template catalog not found. Looked for ${TEMPLATES_RELATIVE_PATH} starting at ${searchFrom}.`,
    )
  }
  const parsed = templatesSchema.safeParse(JSON.parse(readFileSync(found, 'utf8')))
  if (!parsed.success) {
    throw new TemplateError(
      `Template catalog at ${found} failed validation: ${parsed.error.issues
        .map((issue) => `${issue.path.join('.')}: ${issue.message}`)
        .join('; ')}`,
    )
  }
  return parsed.data
}

/**
 * Template registry. Templates supply *defaults* for environment / presence /
 * capability and the module vocabulary; the client config always overrides.
 * No template behaviour is hardcoded here.
 */
export class TemplateRegistry {
  private readonly catalog: TemplateCatalog

  constructor(catalog?: TemplateCatalog) {
    this.catalog = catalog ?? loadTemplates()
  }

  get schemaVersion(): string {
    return this.catalog.schema_version
  }

  get effectiveDate(): string {
    return this.catalog.effective_date
  }

  listNames(): string[] {
    return Object.keys(this.catalog.templates)
  }

  has(name: string): boolean {
    return Object.hasOwn(this.catalog.templates, name)
  }

  /** Throws for an unknown template — never silently falls back. */
  get(name: string): Template {
    const template = this.catalog.templates[name]
    if (!template) {
      throw new TemplateError(
        `Unknown template "${name}". Available templates: ${this.listNames().join(', ')}.`,
      )
    }
    return template
  }

  tryGet(name: string): Template | undefined {
    return this.catalog.templates[name]
  }

  get environmentDefinitions(): Record<string, string> {
    return this.catalog.environment_definitions
  }

  get presenceDefinitions(): Record<string, string> {
    return this.catalog.presence_definitions
  }

  get capabilityDefinitions(): Record<string, string> {
    return this.catalog.capability_definitions
  }

  get selectionRule(): string {
    return this.catalog.selection_rule
  }
}
