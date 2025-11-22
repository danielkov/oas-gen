## 0. High-level shape

For each OpenAPI **tag** (e.g. `pets`), generate a module:

```rust
// feature flag, based on tags
pub  mod pets {
    // service trait
    pub trait PetService<S> {
        async fn list_pets(
            &self,
            ctx: RequestContext<S>,
            query: types::ListPetsQuery,
        ) -> ListPetsResult;

        async fn update_pet(
            &self,
            ctx: RequestContext<S>,
            auth: Option<AuthBearer>,
            id: uuid::Uuid,
            body: types::UpdatePetRequest,
        ) -> UpdatePetResult;
    }

    // router + extension trait
    pub fn router<S, H>(service: H) -> axum::Router<S> { /* ... */ }
    pub trait PetServiceRouterExt<S> { fn router(self) -> axum::Router<S>; }
    impl<S, T> PetServiceRouterExt<S> for T where T: PetService<S> { /* ... */ }

    // generated axum handlers (one per operation)
    async fn list_pets_handler<...>(...) -> axum::response::Response { /* ... */ }
    async fn update_pet_handler<...>(...) -> axum::response::Response { /* ... */ }
}
```

User code:

```
use my_api::pets::{PetService, PetServiceRouterExt}; let  app = axum::Router::with_state(app_state)
    .merge(Pets::new().router())
    .merge(Orders::new().router());
```

---

## 1. Shared generated types

### 1.1 `RequestContext`

Generated once per tag-module:

```
pub  struct  RequestContext<S> { pub state: S, pub method: http::Method, pub uri: http::Uri, pub headers: http::HeaderMap, pub extensions: http::Extensions,
} impl<S> RequestContext<S> { pub  fn  state(&self) -> &S {
        &self.state
    }
} impl<S: Clone> RequestContext<S> { pub(crate) fn  from_parts(state: S, parts: http::request::Parts) ->  Self {
        RequestContext {
            state,
            method: parts.method,
            uri: parts.uri,
            headers: parts.headers,
            extensions: parts.extensions,
        }
    }
}
```

- You keep `Request`-like power (headers, extensions) but not the body.
- State is generic and comes from `Router::with_state(app_state)`.

### 1.2 Auth wrappers

From `securitySchemes`:

`` /// For `bearerAuth` (HTTP bearer)  #[derive(Clone, Debug)]  pub  struct  AuthBearer(pub  String); ``

If a route is _not_ protected by bearer, `auth: Option<AuthBearer>` argument becomes `None` in the handler. If it _is_ required, you can pass `AuthBearer` (non-optional) and fail in the handler if header is missing.

---

## 2. Operation mapping rules

For each OpenAPI operation:

### 2.1 Determine success & error shapes

- Let `SuccessResponses` = all 2xx status codes in the operation.
- Let `ErrorResponses` = all non-2xx status codes.

**Case A – single success response with JSON body**

Example: `200` with `PetList`.

Generate:

`pub  type  ListPetsResult = Result<types::PetList, ListPetsError>;`

**Case B – multiple success responses and/or different schemas**

Example: `200` vs `201`, different schemas, or multiple content-types.

Generate:

`pub  enum  CreatePetSuccess { Created201(types::Pet), // 201  Accepted202(types::AsyncJobInfo), // 202 } pub  type  CreatePetResult = Result<CreatePetSuccess, CreatePetError>;`

**Errors**

For each error response (e.g. 400 / 401 / 404):

```
#[derive(Debug)]  pub  enum  ListPetsError { /// 400: Invalid request parameters  BadRequest400(types::Error),
} #[derive(Debug)]  pub  enum  UpdatePetError { /// 400: Invalid request  BadRequest400(types::Error), /// 401: Unauthorized  Unauthorized401(types::Error),
}
```

You can name variants `Status{code}{Name}` – e.g. `BadRequest400`, `Unauthorized401`, `NotFound404`, etc., generated from status description.

### 2.2 `IntoResponse` for errors and success enums

Errors:

```
impl  axum::response::IntoResponse for  ListPetsError { fn  into_response(self) -> axum::response::Response { use axum::{response::IntoResponse, Json}; use http::StatusCode; match  self {
            ListPetsError::BadRequest400(err) => {
                (StatusCode::BAD_REQUEST, Json(err)).into_response()
            }
        }
    }
}
```

Errors with multiple variants:

```
impl  axum::response::IntoResponse for  UpdatePetError { fn  into_response(self) -> axum::response::Response { use axum::{response::IntoResponse, Json}; use http::StatusCode; match  self {
            UpdatePetError::BadRequest400(err) => {
                (StatusCode::BAD_REQUEST, Json(err)).into_response()
            }
            UpdatePetError::Unauthorized401(err) => {
                (StatusCode::UNAUTHORIZED, Json(err)).into_response()
            }
        }
    }
}
```

Success enums (when needed):

```
impl  axum::response::IntoResponse for  CreatePetSuccess { fn  into_response(self) -> axum::response::Response { use axum::{response::IntoResponse, Json}; use http::StatusCode; match  self {
            CreatePetSuccess::Created201(pet) => {
                (StatusCode::CREATED, Json(pet)).into_response()
            }
            CreatePetSuccess::Accepted202(job) => {
                (StatusCode::ACCEPTED, Json(job)).into_response()
            }
        }
    }
}
```

---

## 3. Trait method signatures

### 3.1 Argument ordering

Define a simple rule (so generation is predictable):

1.  `ctx: RequestContext<S>`
2.  Auth arguments (one per security scheme, optional or required): e.g. `auth: Option<AuthBearer>`
3.  Path params (each as separate argument)
4.  Query param struct (if any)
5.  Header param struct (if any)
6.  Cookie param struct (if any)
7.  Request body (if any)

### 3.2 Examples

#### Example 1: `GET /pets` with query params only

OpenAPI (simplified):

- Path: `/pets`
- Method: `get`
- OperationId: `listPets`
- Query params: `limit`, `offset`, `status`
- Responses:

  - `200` → `PetList`
  - `400` → `Error`

Generated:

```
pub  mod types { use serde::{Deserialize, Serialize}; #[derive(Deserialize)]  pub  struct  ListPetsQuery { pub limit: Option<i32>, pub offset: Option<i32>, pub status: Option<PetStatus>,
    } #[derive(Serialize, Deserialize, Clone)]  pub  struct  PetList { /* ... */ } #[derive(Serialize, Deserialize, Clone)]  pub  enum  PetStatus { /* ... */ } #[derive(Serialize, Deserialize, Clone)]  pub  struct  Error { pub code: String, pub message: String,
    }
} // results / errors  pub  type  ListPetsResult = Result<types::PetList, ListPetsError>; #[derive(Debug)]  pub  enum  ListPetsError { /// 400: Invalid request parameters  BadRequest400(types::Error),
} impl  axum::response::IntoResponse for  ListPetsError { fn  into_response(self) -> axum::response::Response { use axum::{response::IntoResponse, Json}; use http::StatusCode; match  self {
            ListPetsError::BadRequest400(err) => {
                (StatusCode::BAD_REQUEST, Json(err)).into_response()
            }
        }
    }
}
```

Trait method:

```
pub  trait  PetService<S> { async  fn  list_pets(
        &self,
        ctx: RequestContext<S>,
        query: types::ListPetsQuery,
    ) -> ListPetsResult;
}
```

User implementation:

```
impl PetService<AppState> for  Pets { async  fn  list_pets(
        &self,
        ctx: RequestContext<AppState>,
        query: types::ListPetsQuery,
    ) -> ListPetsResult { let  db = &ctx.state.db; let  pets = db.pets.list(
            query.limit.unwrap_or(20),
            query.offset.unwrap_or(0),
            query.status,
        ).map_err(|db_err| { // map to schema-defined Error  let  error = types::Error {
                code: "db_error".into(),
                message: format!("Database error: {db_err}"),
            };
            ListPetsError::BadRequest400(error)
        })?; Ok(pets)
    }
}
```

#### Example 2: `PUT /pets/{id}` with bearer auth + path + JSON body

OpenAPI (simplified):

- Path: `/pets/{id}`
- Method: `put`
- `security: [{ bearerAuth: [] }]`
- Path param: `id: string (uuid)`
- Request body: `UpdatePetRequest`
- Responses:

  - `200` → `Pet`
  - `400` → `Error`
  - `401` → `Error`

Generated types:

```
pub  mod types { use serde::{Deserialize, Serialize}; use uuid::Uuid; #[derive(Deserialize)]  pub  struct  UpdatePetRequest { pub name: String, pub status: Option<PetStatus>,
    } #[derive(Serialize, Deserialize, Clone)]  pub  struct  Pet { pub id: Uuid, pub name: String, pub status: PetStatus,
    } #[derive(Serialize, Deserialize, Clone)]  pub  enum  PetStatus { Available, Pending, Sold } #[derive(Serialize, Deserialize, Clone)]  pub  struct  Error { pub code: String, pub message: String,
    }
} #[derive(Clone, Debug)]  pub  struct  AuthBearer(pub  String); pub  type  UpdatePetResult = Result<types::Pet, UpdatePetError>; #[derive(Debug)]  pub  enum  UpdatePetError { /// 400: Invalid request  BadRequest400(types::Error), /// 401: Unauthorized  Unauthorized401(types::Error),
} impl  axum::response::IntoResponse for  UpdatePetError { fn  into_response(self) -> axum::response::Response { use axum::{response::IntoResponse, Json}; use http::StatusCode; match  self {
            UpdatePetError::BadRequest400(err) => {
                (StatusCode::BAD_REQUEST, Json(err)).into_response()
            }
            UpdatePetError::Unauthorized401(err) => {
                (StatusCode::UNAUTHORIZED, Json(err)).into_response()
            }
        }
    }
}
```

Trait method:

```
pub  trait  PetService<S> { // ... list_pets as before ...  async  fn  update_pet(
        &self,
        ctx: RequestContext<S>,
        auth: AuthBearer, // required; generator knows from security id: uuid::Uuid,
        body: types::UpdatePetRequest,
    ) -> UpdatePetResult;
}
```

Generated handler:

```
async  fn  update_pet_handler<S, H>(
    axum::extract::State(state): axum::extract::State<S>,
    axum::Extension(service): axum::Extension<H>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
    axum::extract::TypedHeader(
        headers::Authorization(headers::authorization::Bearer { token }),
    ): axum::extract::TypedHeader<
        headers::Authorization<headers::authorization::Bearer>,
    >,
    axum::Json(body): axum::Json<types::UpdatePetRequest>,
    req: axum::http::Request<axum::body::Body>,
) -> axum::response::Response where S: Clone + Send + Sync + 'static,
    H: PetService<S> + Clone + Send + Sync + 'static,
{ use axum::response::IntoResponse; let (parts, _body) = req.into_parts(); let  ctx = RequestContext::from_parts(state, parts); let  auth = AuthBearer(token.to_string()); match service.update_pet(ctx, auth, id, body).await { Ok(pet) => { use axum::Json; use http::StatusCode;
            (StatusCode::OK, Json(pet)).into_response()
        } Err(e) => e.into_response(),
    }
}
```

User implementation:

```
impl  PetService<AppState> for  Pets { async  fn  update_pet(
        &self,
        ctx: RequestContext<AppState>,
        auth: AuthBearer,
        id: uuid::Uuid,
        body: types::UpdatePetRequest,
    ) -> UpdatePetResult { let  db = &ctx.state.db; // validate token  let  user_id = match  self.auth.validate_token(&auth.0) { Ok(user_id) => user_id, Err(_) => { let  err = types::Error {
                    code: "invalid_token".into(),
                    message: "Invalid bearer token".into(),
                }; return  Err(UpdatePetError::Unauthorized401(err));
            }
        }; // update logic  let  pet = db.pets.update(user_id, id, body).map_err(|e| { let  err = types::Error {
                code: "update_failed".into(),
                message: format!("Failed to update pet: {e}"),
            };
            UpdatePetError::BadRequest400(err)
        })?; Ok(pet)
    }
}
```

#### Example 3: Path + query + body, no auth

Suppose:

- `POST /search/{kind}?page=...`
- Path param: `kind: string`
- Query params: `page`, `page_size`
- Body: JSON `SearchFilters`
- Responses:

  - `200` → `SearchResults`
  - `400` → `Error`

Trait signature:

```
pub  trait  SearchService<S> { async  fn  search(
        &self,
        ctx: RequestContext<S>,
        kind: String,
        query: types::SearchQueryParams, // generated struct body: types::SearchFilters,
    ) -> SearchResult;
}
```

Generated handler:

```
async  fn  search_handler<S, H>(
    axum::extract::State(state): axum::extract::State<S>,
    axum::Extension(service): axum::Extension<H>,
    axum::extract::Path(kind): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<types::SearchQueryParams>,
    axum::Json(body): axum::Json<types::SearchFilters>,
    req: axum::http::Request<axum::body::Body>,
) -> axum::response::Response where S: Clone + Send + Sync + 'static,
    H: SearchService<S> + Clone + Send + Sync + 'static,
{ use axum::response::IntoResponse; use axum::Json; use http::StatusCode; let (parts, _body) = req.into_parts(); let  ctx = RequestContext::from_parts(state, parts); match service.search(ctx, kind, query, body).await { Ok(results) => (StatusCode::OK, Json(results)).into_response(), Err(e) => e.into_response(),
    }
}
```

---

## 4. Router generation

For each tag module (e.g. `pets`), generate:

```
pub fn router<S, H>(service: H) -> axum::Router<S> where S: Clone + Send + Sync + 'static,
H: PetService<S> + Clone + Send + Sync + 'static,
{ use axum::routing::{get, put}; use axum::{Extension, Router};

    Router::new()
        .route("/pets", get(list_pets_handler::<S, H>))
        .route("/pets/:id", put(update_pet_handler::<S, H>))
        .layer(Extension(service))

}
```

Then generate the extension trait:

```
pub  trait  PetServiceRouterExt<S>: PetService<S> + Clone + Send + Sync + 'static  where S: Clone + Send + Sync + 'static,
{ fn  router(self) -> axum::Router<S> {
        router::<S, Self>(self)
    }
} impl<S, T> PetServiceRouterExt<S> for  T  where S: Clone + Send + Sync + 'static,
    T: PetService<S> + Clone + Send + Sync + 'static,
{
}
```

User:

```
use my_api::pets::PetServiceRouterExt; struct  AppState {
    db: Db,
} struct  Pets {
    db: Db,
    auth: AuthService,
} impl  Pets { fn  new(db: Db, auth: AuthService) ->  Self { Self { db, auth }
    }
} // impl PetService<AppState> for Pets { ... }  let  state = AppState { db: db.clone() }; let  app = axum::Router::with_state(state)
    .merge(Pets::new(db, auth).router());
```

---

## 5. Summary of generator rules

1.  **Module layout**

    - One module per tag (`pets`, `orders`, …).
    - Inside: `types` submodule, `RequestContext`, auth wrappers, per-operation result/error types, trait, router, handlers.

2.  **Traits**

    - Native `async fn` traits (no `async_trait`).
    - Method arguments:
      `ctx` → auth(s) → path params → query struct → header struct → cookie struct → body.
    - Return type:

      - `Result<T, E>` if single success response with single schema.
      - `Result<SuccessEnum, E>` otherwise.

3.  **Errors**

    - Per operation, error enum with one variant per non-2xx response.
    - Variant name encodes HTTP code (`BadRequest400`, `Unauthorized401`, etc.).
    - `IntoResponse` implemented by matching variants and returning `(StatusCode, Json(schema_value))`.

4.  **Handlers**

    - Use Axum extractors:

      - `State<S>`
      - `Extension<H>`
      - `Path<T>`
      - `Query<Struct>`
      - `Json<Body>`
      - `TypedHeader<Authorization<Bearer>>` for bearer auth

    - Build `RequestContext` from `State` + `Request::into_parts()`.
    - Call trait method and map:

      - `Ok(body)` → appropriate status code and JSON.
      - `Err(e)` → `e.into_response()`.

5.  **Router ergonomics**

    - Generate `fn router<S, H>(service: H) -> Router<S>`.
    - Generate `*ServiceRouterExt` trait with `fn router(self) -> Router<S>`.
    - User uses `Pets::new().router()` and `app.merge(...)`.

If you want, next step I can help you sketch the actual codegen templates (e.g. how to format identifiers, where to put lifetimes, which imports each module needs) – but this spec should be enough to start implementing the generator logic.
