# File Upload

AFast supports `multipart/form-data` file upload via the `multer` crate. Two extraction modes are available:

## Raw Multipart (`Multipart`)

For direct access to the multipart stream:

```rust
use afast::{post, Multipart, HttpResult, Json, Tag};
use serde::Serialize;

#[derive(Serialize, Tag)]
#[tag("Upload result")]
struct UploadResult {
    filename: String,
    size: usize,
}

#[post(desc("Upload file"))]
async fn upload(mut form: Multipart) -> HttpResult<Json<UploadResult>> {
    let field = form.next_field().await?.ok_or_else(|| {
        afast::Error::custom(400, "no file provided")
    })?;
    let filename = field.file_name().unwrap_or("unknown").to_string();
    let data = field.bytes().await?;
    Ok(Json(UploadResult { filename, size: data.len() }))
}
```

## Typed Extraction (`MultipartForm<T>`)

For automatic extraction into a struct using `#[derive(FromFormData)]`:

```rust
use afast::{post, MultipartForm, FileField, HttpResult, Json, Tag, FromFormData};
use serde::Serialize;

#[derive(FromFormData, Tag)]
#[tag("Upload form")]
struct UploadForm {
    description: String,
    file: FileField,
}

#[derive(Serialize, Tag)]
#[tag("Upload result")]
struct UploadResult {
    filename: String,
    description: String,
    size: usize,
}

#[post(desc("Upload with form data"))]
async fn upload_typed(form: MultipartForm<UploadForm>) -> HttpResult<Json<UploadResult>> {
    let data = form.0;
    Ok(Json(UploadResult {
        filename: data.file.filename.unwrap_or_else(|| "unknown".to_string()),
        description: data.description,
        size: data.file.bytes.len(),
    }))
}
```

## `#[derive(FromFormData)]`

The derive macro automatically implements the `FromFormData` trait. Each struct field name must match the corresponding form field name.

### Supported Field Types

| Rust Type | Form Field | Notes |
|-----------|-----------|-------|
| `String` | Text field | Direct text value |
| `i8`/`i16`/`i32`/`i64`/`u8`/`u16`/`u32`/`u64`/`f32`/`f64` | Text field | Parsed from text |
| `bool` | Text field | `"true"`/`"false"`/`"1"`/`"0"` |
| `FileField` | File field | Collects bytes, filename, content type |
| `Option<T>` | Optional field | Defaults to `None` if missing |

### `FileField` Structure

```rust
pub struct FileField {
    pub name: String,                // Form field name
    pub filename: Option<String>,    // Original filename
    pub content_type: Option<String>, // MIME type
    pub bytes: Vec<u8>,              // File content
}
```

## Client Code Generation

The TS/JS/KT code generators automatically produce `FormData`-based upload code:

```typescript
// Generated TypeScript client
const formData = new FormData();
formData.append('description', 'My file');
formData.append('file', new Blob(['content']), 'test.txt');
const result = await client.apis.upload_typed({ body: formData });
```

## Testing with curl

```bash
# Raw upload
curl -F "file=@test.txt" http://localhost:5001/upload

# Typed upload with multiple fields
curl -F "description=My file" -F "file=@test.txt" http://localhost:5001/upload/typed
```
