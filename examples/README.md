# Example OpenAPI Specifications

## Pet Store API (`petstore.json`)

A comprehensive example demonstrating various OpenAPI 3.x features:

### Resources

- **Pets**: CRUD operations for pet management
- **Owners**: Customer/owner registration and management
- **Orders**: Order placement and tracking

### Features Demonstrated

#### Types & Schemas

- Complex nested objects (`Pet`, `Owner`, `Order`)
- Enumerations (`Species`, `PetStatus`, `OrderStatus`)
- Optional and nullable fields
- Array and collection types
- Multiple data formats (UUID, date-time, email, URI)

#### Operations

- GET (list and retrieve)
- POST (create)
- PUT (update)
- DELETE (remove)
- Path parameters (`/pets/{petId}`)
- Query parameters (pagination, filtering)
- Request bodies
- Multiple response codes

#### API Design

- Pagination support (`limit`, `offset`)
- Tag-based organization
- Comprehensive documentation
- Multiple servers (production, staging)

### Generate the SDK

```bash
# TypeScript SDK with per-service organization
./target/release/oas-gen examples/petstore.json -t typescript

# Single client style
./target/release/oas-gen examples/petstore.json -t typescript --service-style single-client -o my-sdk

# With verbose output
./target/release/oas-gen examples/petstore.json -t typescript -v
```

### Generated Structure

```
petstore-typescript/
├── package.json          # NPM package configuration
├── tsconfig.json        # TypeScript configuration
└── src/
    ├── index.ts         # Main entry point
    ├── types/
    │   └── index.ts     # Type definitions (13 types)
    └── services/
        ├── pets.ts      # Pet management (5 operations)
        ├── owners.ts    # Owner management (2 operations)
        └── orders.ts    # Order management (1 operation)
```

### API Statistics

- **13 Types**: Pet, Owner, Order, Address, Error, NewPet, NewOwner, NewOrder, UpdatePet, PetList, Species (enum), PetStatus (enum), OrderStatus (enum)
- **3 Services**: Pets, Owners, Orders
- **8 Operations**: listPets, createPet, getPetById, updatePet, deletePet, listOwners, createOwner, createOrder
