use std::path::PathBuf;

use crate::protocol::AppErrorDto;
use manganis::jni::JNIEnv;
use manganis::jni::objects::{JObject, JValue};

const CONTENT_SCHEME: &str = "content://";

pub async fn write_document(
    existing_target: Option<String>,
    suggested_name: String,
    bytes: Vec<u8>,
) -> Result<Option<String>, AppErrorDto> {
    tokio::task::spawn_blocking(move || {
        with_activity(|env, activity| {
            let resolver = env
                .call_method(
                    activity,
                    "getContentResolver",
                    "()Landroid/content/ContentResolver;",
                    &[],
                )
                .map_err(jni_error)?
                .l()
                .map_err(jni_error)?;

            if let Some(target) = existing_target.filter(|target| is_content_uri(target)) {
                write_existing(env, &resolver, &target, &bytes)?;
                return Ok(Some(target));
            }

            let name = safe_file_name(&suggested_name);
            let uri = insert_download(env, &resolver, &name)?;
            if let Err(error) = write_uri(env, &resolver, &uri, &bytes, "w") {
                delete_uri(env, &resolver, &uri);
                return Err(error);
            }
            if let Err(error) = finish_download(env, &resolver, &uri) {
                delete_uri(env, &resolver, &uri);
                return Err(error);
            }
            let uri = object_to_string(env, &uri)?;
            Ok(Some(uri))
        })
    })
    .await
    .map_err(|error| android_error(format!("Android file task failed: {error}")))?
}

pub fn app_files_dir() -> Result<PathBuf, AppErrorDto> {
    with_activity(|env, activity| {
        let directory = env
            .call_method(activity, "getFilesDir", "()Ljava/io/File;", &[])
            .map_err(jni_error)?
            .l()
            .map_err(jni_error)?;
        if directory.is_null() {
            return Err("Android app files directory is unavailable".to_string());
        }
        object_to_string_method(env, &directory, "getAbsolutePath").map(PathBuf::from)
    })
}

fn with_activity<T>(
    operation: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> Result<T, String>,
) -> Result<T, AppErrorDto> {
    manganis::android::with_activity(|env, activity| Some(operation(env, activity)))
        .ok_or_else(|| android_error("Android activity is unavailable"))?
        .map_err(android_error)
}

fn insert_download<'local>(
    env: &mut JNIEnv<'local>,
    resolver: &JObject<'local>,
    name: &str,
) -> Result<JObject<'local>, String> {
    let downloads = env
        .find_class("android/provider/MediaStore$Downloads")
        .map_err(jni_error)?;
    let collection = env
        .get_static_field(downloads, "EXTERNAL_CONTENT_URI", "Landroid/net/Uri;")
        .map_err(jni_error)?
        .l()
        .map_err(jni_error)?;
    let values = env
        .new_object("android/content/ContentValues", "()V", &[])
        .map_err(jni_error)?;
    put_string(env, &values, "_display_name", name)?;
    put_string(env, &values, "mime_type", mime_type(name))?;
    put_string(env, &values, "relative_path", "Download/Simple Table")?;
    put_integer(env, &values, "is_pending", 1)?;

    let uri = env
        .call_method(
            resolver,
            "insert",
            "(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;",
            &[JValue::Object(&collection), JValue::Object(&values)],
        )
        .map_err(jni_error)?
        .l()
        .map_err(jni_error)?;
    if uri.is_null() {
        return Err("Android MediaStore refused to create the document".to_string());
    }
    Ok(uri)
}

fn write_existing(
    env: &mut JNIEnv<'_>,
    resolver: &JObject<'_>,
    target: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let target = env.new_string(target).map_err(jni_error)?;
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&target)],
        )
        .map_err(jni_error)?
        .l()
        .map_err(jni_error)?;
    write_uri(env, resolver, &uri, bytes, "wt")
}

fn write_uri(
    env: &mut JNIEnv<'_>,
    resolver: &JObject<'_>,
    uri: &JObject<'_>,
    bytes: &[u8],
    mode: &str,
) -> Result<(), String> {
    let mode = env.new_string(mode).map_err(jni_error)?;
    let stream = env
        .call_method(
            resolver,
            "openOutputStream",
            "(Landroid/net/Uri;Ljava/lang/String;)Ljava/io/OutputStream;",
            &[JValue::Object(uri), JValue::Object(&mode)],
        )
        .map_err(jni_error)?
        .l()
        .map_err(jni_error)?;
    if stream.is_null() {
        return Err("Android could not open the selected document".to_string());
    }

    let data = env.byte_array_from_slice(bytes).map_err(jni_error)?;
    let data = JObject::from(data);
    let result = env
        .call_method(&stream, "write", "([B)V", &[JValue::Object(&data)])
        .and_then(|_| env.call_method(&stream, "flush", "()V", &[]))
        .map_err(jni_error);
    let close_result = env
        .call_method(&stream, "close", "()V", &[])
        .map_err(jni_error);
    result?;
    close_result?;
    Ok(())
}

fn finish_download(
    env: &mut JNIEnv<'_>,
    resolver: &JObject<'_>,
    uri: &JObject<'_>,
) -> Result<(), String> {
    let values = env
        .new_object("android/content/ContentValues", "()V", &[])
        .map_err(jni_error)?;
    put_integer(env, &values, "is_pending", 0)?;
    let updated = env
        .call_method(
            resolver,
            "update",
            "(Landroid/net/Uri;Landroid/content/ContentValues;Ljava/lang/String;[Ljava/lang/String;)I",
            &[
                JValue::Object(uri),
                JValue::Object(&values),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
            ],
        )
        .map_err(jni_error)?
        .i()
        .map_err(jni_error)?;
    if updated != 1 {
        return Err("Android MediaStore did not finalize the document".to_string());
    }
    Ok(())
}

fn delete_uri(env: &mut JNIEnv<'_>, resolver: &JObject<'_>, uri: &JObject<'_>) {
    let _ = env.call_method(
        resolver,
        "delete",
        "(Landroid/net/Uri;Ljava/lang/String;[Ljava/lang/String;)I",
        &[
            JValue::Object(uri),
            JValue::Object(&JObject::null()),
            JValue::Object(&JObject::null()),
        ],
    );
}

fn put_string(
    env: &mut JNIEnv<'_>,
    values: &JObject<'_>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let key = env.new_string(key).map_err(jni_error)?;
    let value = env.new_string(value).map_err(jni_error)?;
    env.call_method(
        values,
        "put",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[JValue::Object(&key), JValue::Object(&value)],
    )
    .map_err(jni_error)?;
    Ok(())
}

fn put_integer(
    env: &mut JNIEnv<'_>,
    values: &JObject<'_>,
    key: &str,
    value: i32,
) -> Result<(), String> {
    let key = env.new_string(key).map_err(jni_error)?;
    let value = env
        .new_object("java/lang/Integer", "(I)V", &[JValue::Int(value)])
        .map_err(jni_error)?;
    env.call_method(
        values,
        "put",
        "(Ljava/lang/String;Ljava/lang/Integer;)V",
        &[JValue::Object(&key), JValue::Object(&value)],
    )
    .map_err(jni_error)?;
    Ok(())
}

fn object_to_string(env: &mut JNIEnv<'_>, object: &JObject<'_>) -> Result<String, String> {
    object_to_string_method(env, object, "toString")
}

fn object_to_string_method(
    env: &mut JNIEnv<'_>,
    object: &JObject<'_>,
    method: &str,
) -> Result<String, String> {
    let value = env
        .call_method(object, method, "()Ljava/lang/String;", &[])
        .map_err(jni_error)?
        .l()
        .map_err(jni_error)?;
    let value = manganis::jni::objects::JString::from(value);
    env.get_string(&value)
        .map(|value| value.into())
        .map_err(jni_error)
}

fn is_content_uri(target: &str) -> bool {
    target.starts_with(CONTENT_SCHEME)
}

fn safe_file_name(name: &str) -> String {
    let name = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | '\0' => '_',
            other => other,
        })
        .collect::<String>();
    let name = name.trim();
    if name.is_empty() {
        "workbook.xlsx".to_string()
    } else {
        name.to_string()
    }
}

fn mime_type(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".csv") {
        "text/csv"
    } else if name.ends_with(".xlsm") {
        "application/vnd.ms-excel.sheet.macroEnabled.12"
    } else {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    }
}

fn jni_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn android_error(message: impl Into<String>) -> AppErrorDto {
    AppErrorDto {
        code: "android_file_error".to_string(),
        message: message.into(),
    }
}
