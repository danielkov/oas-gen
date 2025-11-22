//! Integration tests for the code generation pipeline.

use codegen::{Config, ServiceStyle, VirtualFS};
use generate::GeneratorRegistry;
use ir::gen_ir::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use typescript::TypeScriptGenerator;

/// Helper to create a minimal GenIr for testing.
fn create_test_ir() -> GenIr {
    let api = ApiMeta {
        title: "Test API".to_string(),
        version: "1.0.0".to_string(),
        package_name: CanonicalName {
            canonical: "TestApi".to_string(),
            snake: "test_api".to_string(),
            camel: "testApi".to_string(),
            pascal: "TestApi".to_string(),
            kebab: "test-api".to_string(),
            upper: "TEST_API".to_string(),
            lower: "testapi".to_string(),
        },
        docs: Docs {
            summary: Some("A test API".to_string()),
            description: Some("This is a test API for demonstration".to_string()),
            deprecated: false,
            since: None,
            examples: vec![],
            external_urls: vec![],
        },
    };

    let mut types = BTreeMap::new();

    // Add a simple struct type
    let user_type = TypeDecl {
        id: StableId("User".to_string()),
        name: CanonicalName {
            canonical: "User".to_string(),
            snake: "user".to_string(),
            camel: "user".to_string(),
            pascal: "User".to_string(),
            kebab: "user".to_string(),
            upper: "USER".to_string(),
            lower: "user".to_string(),
        },
        docs: Docs {
            summary: Some("A user entity".to_string()),
            description: None,
            deprecated: false,
            since: None,
            examples: vec![],
            external_urls: vec![],
        },
        kind: TypeKind::Struct {
            fields: vec![
                Field {
                    name: CanonicalName {
                        canonical: "Id".to_string(),
                        snake: "id".to_string(),
                        camel: "id".to_string(),
                        pascal: "Id".to_string(),
                        kebab: "id".to_string(),
                        upper: "ID".to_string(),
                        lower: "id".to_string(),
                    },
                    docs: Docs {
                        summary: Some("User ID".to_string()),
                        ..Default::default()
                    },
                    ty: TypeRef {
                        target: StableId("String".to_string()),
                        optional: false,
                        nullable: false,
                        by_ref: false,
                        modifiers: vec![],
                    },
                    default: None,
                    deprecated: false,
                    const_value: None,
                    wire_name: "id".to_string(),
                },
                Field {
                    name: CanonicalName {
                        canonical: "Name".to_string(),
                        snake: "name".to_string(),
                        camel: "name".to_string(),
                        pascal: "Name".to_string(),
                        kebab: "name".to_string(),
                        upper: "NAME".to_string(),
                        lower: "name".to_string(),
                    },
                    docs: Docs {
                        summary: Some("User name".to_string()),
                        ..Default::default()
                    },
                    ty: TypeRef {
                        target: StableId("String".to_string()),
                        optional: false,
                        nullable: false,
                        by_ref: false,
                        modifiers: vec![],
                    },
                    default: None,
                    deprecated: false,
                    const_value: None,
                    wire_name: "name".to_string(),
                },
                Field {
                    name: CanonicalName {
                        canonical: "Email".to_string(),
                        snake: "email".to_string(),
                        camel: "email".to_string(),
                        pascal: "Email".to_string(),
                        kebab: "email".to_string(),
                        upper: "EMAIL".to_string(),
                        lower: "email".to_string(),
                    },
                    docs: Docs {
                        summary: Some("User email".to_string()),
                        ..Default::default()
                    },
                    ty: TypeRef {
                        target: StableId("String".to_string()),
                        optional: true,
                        nullable: false,
                        by_ref: false,
                        modifiers: vec![],
                    },
                    default: None,
                    deprecated: false,
                    const_value: None,
                    wire_name: "email".to_string(),
                },
            ],
            additional: Additional::Forbidden,
            discriminator: None,
        },
        origin: None,
        tags: BTreeSet::new(),
    };

    types.insert(user_type.id.clone(), user_type);

    // Add an enum type
    let status_type = TypeDecl {
        id: StableId("Status".to_string()),
        name: CanonicalName {
            canonical: "Status".to_string(),
            snake: "status".to_string(),
            camel: "status".to_string(),
            pascal: "Status".to_string(),
            kebab: "status".to_string(),
            upper: "STATUS".to_string(),
            lower: "status".to_string(),
        },
        docs: Docs {
            summary: Some("User status".to_string()),
            ..Default::default()
        },
        kind: TypeKind::Enum {
            base: Primitive::String,
            values: vec![
                EnumValue {
                    name: CanonicalName {
                        canonical: "Active".to_string(),
                        snake: "active".to_string(),
                        camel: "active".to_string(),
                        pascal: "Active".to_string(),
                        kebab: "active".to_string(),
                        upper: "ACTIVE".to_string(),
                        lower: "active".to_string(),
                    },
                    docs: Docs::default(),
                    wire: Literal::String("active".to_string()),
                },
                EnumValue {
                    name: CanonicalName {
                        canonical: "Inactive".to_string(),
                        snake: "inactive".to_string(),
                        camel: "inactive".to_string(),
                        pascal: "Inactive".to_string(),
                        kebab: "inactive".to_string(),
                        upper: "INACTIVE".to_string(),
                        lower: "inactive".to_string(),
                    },
                    docs: Docs::default(),
                    wire: Literal::String("inactive".to_string()),
                },
            ],
        },
        origin: None,
        tags: BTreeSet::new(),
    };

    types.insert(status_type.id.clone(), status_type);

    // Add a simple service with one operation
    let service = Service {
        id: StableId("UserService".to_string()),
        name: CanonicalName {
            canonical: "UserService".to_string(),
            snake: "user_service".to_string(),
            camel: "userService".to_string(),
            pascal: "UserService".to_string(),
            kebab: "user-service".to_string(),
            upper: "USER_SERVICE".to_string(),
            lower: "userservice".to_string(),
        },
        docs: Docs {
            summary: Some("User management service".to_string()),
            ..Default::default()
        },
        server_set: None,
        operations: vec![Operation {
            id: StableId("GetUser".to_string()),
            name: CanonicalName {
                canonical: "GetUser".to_string(),
                snake: "get_user".to_string(),
                camel: "getUser".to_string(),
                pascal: "GetUser".to_string(),
                kebab: "get-user".to_string(),
                upper: "GET_USER".to_string(),
                lower: "getuser".to_string(),
            },
            docs: Docs {
                summary: Some("Get a user by ID".to_string()),
                ..Default::default()
            },
            deprecated: false,
            http: HttpShape {
                method: HttpMethod::Get,
                path_template: "/users/{id}".to_string(),
                segments: vec![],
                query: vec![],
                headers: vec![],
                cookies: vec![],
                path_params: vec![],
                body: None,
                consumes: vec![],
                produces: vec!["application/json".to_string()],
            },
            success: None,
            alt_success: vec![],
            errors: ErrorUse::None,
            auth: vec![],
            pagination: None,
            idempotent: true,
            retryable_statuses: BTreeSet::new(),
        }],
    };

    GenIr {
        api,
        types,
        services: vec![service],
        auth_schemes: vec![],
        errors: vec![],
        server_sets: vec![],
    }
}

#[test]
fn test_virtual_fs_operations() {
    let mut vfs = VirtualFS::new();

    assert!(vfs.is_empty());
    assert_eq!(vfs.len(), 0);

    vfs.add_file("test.txt", "Hello, world!");
    vfs.add_file("dir/nested.txt", "Nested content");

    assert!(!vfs.is_empty());
    assert_eq!(vfs.len(), 2);

    assert!(vfs.contains(Path::new("test.txt")));
    assert!(vfs.contains(Path::new("dir/nested.txt")));

    let content = vfs.get_file_str(Path::new("test.txt")).unwrap().unwrap();
    assert_eq!(content, "Hello, world!");

    let files: Vec<_> = vfs.files().collect();
    assert_eq!(files.len(), 2);
}

#[test]
fn test_registry_default_generators() {
    let mut registry = GeneratorRegistry::with_defaults();

    // Manually register TypeScript generator for testing
    registry.register(Box::new(TypeScriptGenerator::new()));

    let languages = registry.languages();
    assert!(!languages.is_empty());
    assert!(languages.contains(&"typescript"));

    let ts_gen = registry.get("typescript");
    assert!(ts_gen.is_some());
    assert_eq!(ts_gen.unwrap().language(), "typescript");
}

#[test]
fn test_typescript_generation() {
    let test_ir = create_test_ir();
    let config = Config::default();
    let mut registry = GeneratorRegistry::with_defaults();
    registry.register(Box::new(TypeScriptGenerator::new()));

    let result = registry.generate("typescript", &test_ir, &config);
    assert!(result.is_ok(), "Generation failed: {:?}", result.err());

    let vfs = result.unwrap();

    // Check that expected files are generated
    assert!(vfs.contains(Path::new("package.json")));
    assert!(vfs.contains(Path::new("tsconfig.json")));
    assert!(vfs.contains(Path::new("src/types/index.ts")));
    assert!(vfs.contains(Path::new("src/index.ts")));

    // Check package.json content
    let package_json = vfs
        .get_file_str(Path::new("package.json"))
        .unwrap()
        .unwrap();
    assert!(package_json.contains("test-api"));
    assert!(package_json.contains("1.0.0"));

    // Check types are generated
    let types_content = vfs
        .get_file_str(Path::new("src/types/index.ts"))
        .unwrap()
        .unwrap();
    assert!(types_content.contains("interface User"));
    assert!(types_content.contains("enum Status"));
    assert!(types_content.contains("id: "));
    assert!(types_content.contains("name: "));
    assert!(types_content.contains("email?: "));
    assert!(types_content.contains("ACTIVE"));
    assert!(types_content.contains("INACTIVE"));
}

#[test]
fn test_typescript_per_service_style() {
    let test_ir = create_test_ir();
    let config = Config {
        service_style: ServiceStyle::PerService,
        ..Default::default()
    };
    let mut registry = GeneratorRegistry::with_defaults();
    registry.register(Box::new(TypeScriptGenerator::new()));

    let vfs = registry.generate("typescript", &test_ir, &config).unwrap();

    // Check that service file is generated per service
    assert!(vfs.contains(Path::new("src/services/user_service.ts")));

    let service_content = vfs
        .get_file_str(Path::new("src/services/user_service.ts"))
        .unwrap()
        .unwrap();
    assert!(service_content.contains("class UserServiceService"));
    assert!(service_content.contains("async getUser"));
}

#[test]
fn test_typescript_single_client_style() {
    let test_ir = create_test_ir();
    let config = Config {
        service_style: ServiceStyle::SingleClient,
        ..Default::default()
    };
    let mut registry = GeneratorRegistry::with_defaults();
    registry.register(Box::new(TypeScriptGenerator::new()));

    let vfs = registry.generate("typescript", &test_ir, &config).unwrap();

    // Check that single client file is generated
    assert!(vfs.contains(Path::new("src/services/client.ts")));

    let client_content = vfs
        .get_file_str(Path::new("src/services/client.ts"))
        .unwrap()
        .unwrap();
    assert!(client_content.contains("class TestApiSDK"));
    assert!(client_content.contains("get userService"));
}

#[test]
fn test_validation_empty_ir() {
    let empty_ir = GenIr {
        api: ApiMeta {
            title: "Empty".to_string(),
            version: "1.0.0".to_string(),
            package_name: CanonicalName {
                canonical: "Empty".to_string(),
                snake: "empty".to_string(),
                camel: "empty".to_string(),
                pascal: "Empty".to_string(),
                kebab: "empty".to_string(),
                upper: "EMPTY".to_string(),
                lower: "empty".to_string(),
            },
            docs: Docs::default(),
        },
        types: BTreeMap::new(),
        services: vec![],
        auth_schemes: vec![],
        errors: vec![],
        server_sets: vec![],
    };

    let config = Config::default();
    let mut registry = GeneratorRegistry::with_defaults();
    registry.register(Box::new(TypeScriptGenerator::new()));

    let result = registry.generate("typescript", &empty_ir, &config);
    assert!(result.is_err());
}

#[test]
fn test_generator_not_found() {
    let test_ir = create_test_ir();
    let config = Config::default();
    let registry = GeneratorRegistry::with_defaults();

    let result = registry.generate("nonexistent", &test_ir, &config);
    assert!(result.is_err());
}

#[test]
fn test_config_options() {
    let config1 = Config {
        service_style: ServiceStyle::SingleClient,
        include_docs: false,
        lang_options: BTreeMap::new(),
    };

    assert_eq!(config1.service_style, ServiceStyle::SingleClient);
    assert!(!config1.include_docs);

    let config2 = Config::default();
    assert_eq!(config2.service_style, ServiceStyle::PerService);
    assert!(config2.include_docs);
}
