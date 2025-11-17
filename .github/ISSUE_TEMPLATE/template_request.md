---
name: Template request
about: Request support for a new language
title: '[TEMPLATE] Add <language> support'
labels: template, enhancement
assignees: ''
---

## Language
Which language/framework do you want supported?

Example: Python, Rust, Go, Java, C#, Ruby, etc.

## Use case
Why do you need this language? What will you use it for?

## Existing generators
Are there existing OpenAPI generators for this language that could serve as reference?
- Link 1
- Link 2

## Expected output structure
What should the generated code structure look like?

```
my-sdk/
├── types/
│   └── models.py
├── services/
│   ├── pets.py
│   └── users.py
└── setup.py
```

## Code style
Any specific conventions or patterns for this language?
- Naming conventions (snake_case, camelCase, PascalCase)
- Type hints / annotations
- Documentation format
- Testing patterns

## Example
Show an example of what generated code should look like for this operation:

```yaml
# OpenAPI
paths:
  /pets:
    get:
      operationId: listPets
      responses:
        '200':
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Pet'
```

Expected generated code:
```python
# Your expected output here
class PetsService:
    def list_pets(self) -> List[Pet]:
        ...
```

## Dependencies
What runtime dependencies would the generated code need?
- HTTP client library
- JSON parsing
- Type checking
- etc.

## Willing to contribute?
- [ ] Yes, I can implement this template
- [ ] Yes, but I need help getting started
- [ ] No, just requesting

## Additional context
Any other information about the language or requirements.

