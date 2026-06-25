# 文件上传

AFast 通过 `multer` crate 支持 `multipart/form-data` 文件上传。提供两种提取模式：

## 原始 Multipart (`Multipart`)

直接访问 multipart 流：

```rust
use afast::{post, Multipart, HttpResult, Json, Tag};
use serde::Serialize;

#[derive(Serialize, Tag)]
#[tag("上传结果")]
struct UploadResult {
    filename: String,
    size: usize,
}

#[post(desc("上传文件"))]
async fn upload(mut form: Multipart) -> HttpResult<Json<UploadResult>> {
    let field = form.next_field().await?.ok_or_else(|| {
        afast::Error::custom(400, "未提供文件")
    })?;
    let filename = field.file_name().unwrap_or("unknown").to_string();
    let data = field.bytes().await?;
    Ok(Json(UploadResult { filename, size: data.len() }))
}
```

## 类型化提取 (`MultipartForm<T>`)

使用 `#[derive(FromFormData)]` 自动提取到结构体：

```rust
use afast::{post, MultipartForm, FileField, HttpResult, Json, Tag, FromFormData};
use serde::Serialize;

#[derive(FromFormData, Tag)]
#[tag("上传表单")]
struct UploadForm {
    description: String,
    file: FileField,
}

#[derive(Serialize, Tag)]
#[tag("上传结果")]
struct UploadResult {
    filename: String,
    description: String,
    size: usize,
}

#[post(desc("带表单数据的上传"))]
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

该 derive 宏自动实现 `FromFormData` trait。每个结构体字段名必须与对应的表单字段名匹配。

### 支持的字段类型

| Rust 类型 | 表单字段 | 说明 |
|-----------|---------|------|
| `String` | 文本字段 | 直接文本值 |
| `i8`/`i16`/`i32`/`i64`/`u8`/`u16`/`u32`/`u64`/`f32`/`f64` | 文本字段 | 从文本解析 |
| `bool` | 文本字段 | `"true"`/`"false"`/`"1"`/`"0"` |
| `FileField` | 文件字段 | 收集字节、文件名、内容类型 |
| `Option<T>` | 可选字段 | 缺失时默认为 `None` |

### `FileField` 结构

```rust
pub struct FileField {
    pub name: String,                // 表单字段名
    pub filename: Option<String>,    // 原始文件名
    pub content_type: Option<String>, // MIME 类型
    pub bytes: Vec<u8>,              // 文件内容
}
```

## 客户端代码生成

TS/JS/KT 代码生成器自动生成基于 `FormData` 的上传代码：

```typescript
// 生成的 TypeScript 客户端
const formData = new FormData();
formData.append('description', 'My file');
formData.append('file', new Blob(['content']), 'test.txt');
const result = await client.apis.upload_typed({ body: formData });
```

## 使用 curl 测试

```bash
# 原始上传
curl -F "file=@test.txt" http://localhost:5001/upload

# 带多个字段的类型化上传
curl -F "description=我的文件" -F "file=@test.txt" http://localhost:5001/upload/typed
```
