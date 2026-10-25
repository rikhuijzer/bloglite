//! File upload and download at `/files`.
use crate::data::Kv;
use crate::html::PageSettings;
use crate::html::Top;
use crate::html::page;
use crate::serve::RequestResult;
use crate::serve::ServerContext;
use crate::serve::is_logged_in;
use crate::serve::not_found;
use crate::serve::response;
use axum::Router;
use axum::extract::Form;
use axum::extract::Multipart;
use axum::extract::Path;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::routing::get;
use axum::routing::post;
use axum_extra::extract::CookieJar;
use bytes::Bytes;
use rusqlite::Connection;
use rusqlite::Result;
use rusqlite::params;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;

#[derive(Debug, Deserialize, Serialize)]
pub struct File {
    // The sha of the file. This is the primary key. The benefit of this is that
    // we can accept any uploads via the HTML file input since duplicates are
    // simply the same file. Another benefit is that it will work well with
    // having hidden posts later. Thanks to it being a sha, it will be hard to
    // guess the name of the file.
    pub sha: String,
    pub mime_type: String,
    /// Filename is shown for easier identification.
    pub filename: String,
    pub data: Bytes,
}

impl File {
    fn new(mime_type: &str, filename: &str, data: Bytes) -> Self {
        let sha = sha2::Sha256::digest(&data);
        // Turning the 256 bit hash into a 64 bit hash. The probability of a
        // collision is roughly 1 in 2^(n/2) which means 1 in 2^32=4 billion to
        // get a collision. Collisions are not a security risk here, because the
        // site owner is the only one who can upload files. If my math is right,
        // it would take 1 million sites with 1000 files each before a collision
        // is likely to occur. This risk is worth it since a shorter hash is
        // much easier to work with.
        let sha = sha[..8].to_vec();
        let sha = hex::encode(sha);
        Self {
            sha,
            mime_type: mime_type.to_string(),
            filename: filename.to_string(),
            data,
        }
    }
}

fn bytes_to_blob(bytes: &Bytes) -> Vec<u8> {
    bytes.to_vec()
}

fn blob_to_bytes(blob: Vec<u8>) -> Bytes {
    Bytes::from(blob)
}

impl File {
    pub fn create_table(conn: &Connection) -> Result<usize> {
        let stmt = "
            CREATE TABLE IF NOT EXISTS files (
                sha TEXT PRIMARY KEY,
                mime_type TEXT NOT NULL,
                filename TEXT NOT NULL,
                data BLOB NOT NULL
            );
        ";
        conn.execute(stmt, [])
    }
    pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Self>> {
        let stmt = "
            SELECT sha, mime_type, filename, data
            FROM files
            ORDER BY filename;
            ";
        let mut stmt = conn.prepare(stmt)?;
        let files = stmt.query_map([], |row| {
            Ok(File {
                sha: row.get("sha")?,
                mime_type: row.get("mime_type")?,
                filename: row.get("filename")?,
                data: blob_to_bytes(row.get("data")?),
            })
        })?;
        files.collect::<Result<Vec<_>, _>>()
    }
    pub fn insert(conn: &Connection, file: &Self) -> rusqlite::Result<usize> {
        // We can safely use `INSERT OR REPLACE` because the sha is the primary
        // key. In the "worse" case, it will only rename the filename. If the
        // mime type is different, then it means that the extension was changed
        // locally which probably is good to reflect in the database. Also
        // having the need for two identical files with different mimetypes is a
        // very unlikely scenario.
        let sql = "
            INSERT OR REPLACE INTO files (sha, mime_type, filename, data)
            VALUES (?, ?, ?, ?);
            ";
        let data = bytes_to_blob(&file.data);
        let params = params![file.sha, file.mime_type, file.filename, data];
        conn.execute(sql, params)
    }
    pub fn get(conn: &Connection, name: &str) -> rusqlite::Result<Self> {
        let stmt = "
            SELECT sha, mime_type, filename, data
            FROM files
            WHERE sha = ?;
            ";
        let mut stmt = conn.prepare(stmt)?;
        // Not logging an error because it could just mean that someone
        // requested an a file (by requesting some URL) that does not exist.
        stmt.query_row([name], |row| {
            Ok(File {
                sha: row.get("sha")?,
                mime_type: row.get("mime_type")?,
                filename: row.get("filename")?,
                data: blob_to_bytes(row.get("data")?),
            })
        })
    }
    pub fn delete(conn: &Connection, sha: &str) -> rusqlite::Result<usize> {
        let sql = "DELETE FROM files WHERE sha = ?";
        conn.execute(sql, [sha])
    }
    pub fn rename(conn: &Connection, sha: &str, filename: &str) -> rusqlite::Result<usize> {
        let sql = "UPDATE files SET filename = ? WHERE sha = ?";
        conn.execute(sql, [filename, sha])
    }
    // If the filename contains a forward slash, return the part after the
    // forward slash; otherwise, return the filename. When the prefix would be
    // kept, the forward slash would turn the url into
    // /files/sha/prefix/filename, which is not an endpoint that exists since
    // only /files/sha and /files/sha/filename are valid.
    pub fn filename_without_prefix(&self) -> String {
        let filename = &self.filename;
        if let Some(index) = filename.find('/') {
            filename[index + 1..].to_string()
        } else {
            filename.to_string()
        }
    }
}

fn md_link(file: &File) -> String {
    let sha = &file.sha;
    let filename = &file.filename;
    if file.mime_type.starts_with("image/") {
        format!("![{filename}](/files/{sha})")
    } else {
        let filename = file.filename_without_prefix();
        let filename = crate::html::url_encode(&filename);
        format!("[{filename}](/files/{sha}/{filename})")
    }
}

fn show_file(file: &File) -> String {
    let sha = &file.sha;
    let link = md_link(file);
    let filename = file.filename_without_prefix();
    let filename = crate::html::url_encode(&filename);
    format!(
        "
        <div id='file-{sha}' style='padding: 6px; padding-bottom: 0px; padding-top: 12px; \
          border-bottom: 1px solid var(--border); font-size: 0.8rem;'>
            <a href='/files/{sha}/{filename}'>{}</a>&nbsp;&nbsp;
            <a class='unstyled-link' href='/files/rename/{sha}' \
              style='font-size: 0.8rem; padding-top: 0.1rem;'>
                ✏️ Rename
            </a>&nbsp;
            <a class='unstyled-link' href='/files/delete/{sha}' \
              style='font-size: 0.8rem; padding-top: 0.1rem;'>
                🗑️ Delete
            </a><br>
            <span style='font-size: var(--ui-font-size);'>
                Markdown link
                (<a id='copy-{sha}' href='javascript:void(0)' \
                  onclick='copyCode(\"{sha}\")'>copy</a>):
            </span><br>
            <pre style='margin-top: 6px; margin-bottom: 0px;'>
                <code id='code-{sha}' class='language-md'>{link}</code>
            </pre>
        </div>
        ",
        file.filename,
    )
}

async fn get_files(State(ctx): State<ServerContext>, jar: CookieJar) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    let files = File::list(&*ctx.conn()?).unwrap();
    let files = files
        .iter()
        .map(show_file)
        .collect::<Vec<String>>()
        .join("");
    let body = format!(
        "
        <div style='border-bottom: 2px solid var(--border);'>
            <form method='post' action='/files/add' \
              class='margin-auto' \
              enctype='multipart/form-data' \
              style='margin-top: 5vh; width: 80%;'>
                <div>
                    <label for='file'>Choose file(s) to upload (max 15 MB)</label><br>
                    <input type='file' id='file' name='file' multiple required />
                </div>
                <br>
                <div>
                    <label for='prefix'>Prefix for the uploaded file(s) (optional)</label><br>
                    <input type='text' id='prefix' name='prefix' \
                      placeholder='example/'/> 
                    <br>
                    <span style='font-size: 0.8rem; line-height: 1.2; \
                      display: inline-block;'>
                        This is added to the beginning of the filename. For example, set the prefix
                        'example/' to get files like 'example/a.txt' and 'example/b.pdf'.
                    </span>
                </div>
                <br>
                <div>
                    <input style='margin-left: 0;' type='submit' value='Upload'/>
                </div>
                <br>
                <br>
            </form>
        </div>
        <div style='font-size: 0.8rem; padding: 6px; padding-bottom: 10px;'>
            Each file will get an unique SHA identifier, such as <code>69b83ddf8f65695f</code>.
            To link to the file, you can use
            <code>/files/69b83ddf8f65695f</code>,
            <code>/files/69b83ddf8f65695f/example.txt</code>, or
            <code>/files/69b83ddf8f65695f/something-else.txt</code>.
        </div>
        <div>
            {files}
        </div>
        "
    );
    let page_settings =
        PageSettings::new("Files", Some(is_logged_in), None, false, Top::GoHome, "");
    let body = page(&ctx, &page_settings, &body).await?;
    tracing::info!("\"GET /files HTTP/1.1\" 200");
    response(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

async fn get_file(State(ctx): State<ServerContext>, Path(sha): Path<String>) -> RequestResult {
    // Anything after the sha is allowed. This allows anyone to decide the
    // filename. For security purposes, this should be okay since the only
    // person that can upload files is the site owner. The benefit of this is
    // that files can be renamed without breaking links.
    let trim_len = 16;
    let name = if trim_len < sha.len() {
        sha[..trim_len].to_string()
    } else {
        sha.clone()
    };
    let file = match File::get(&*ctx.conn()?, &name) {
        Ok(file) => file,
        Err(_) => return not_found(State(ctx)).await,
    };
    let mut headers = HeaderMap::new();
    crate::serve::content_type(&mut headers, &file.mime_type);
    // Setting this too high might make deleted files accessible for too long
    // which could be confusing for the user.
    let max_age = 300;
    crate::serve::enable_caching(&mut headers, max_age);
    tracing::info!("\"GET /files/{sha} HTTP/1.1\" 200");
    response(StatusCode::OK, headers, file.data, &ctx)
}

async fn get_file_with_filename(
    State(ctx): State<ServerContext>,
    Path((sha, _filename)): Path<(String, String)>,
) -> RequestResult {
    get_file(State(ctx), Path(sha)).await
}

async fn post_file(
    State(ctx): State<ServerContext>,
    jar: CookieJar,
    mut multipart: Multipart,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    let mut received_files = Vec::new();
    let mut prefix = String::new();
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name();
        if name == Some("file") {
            let filename = field.file_name().unwrap().to_string();
            if filename.is_empty() {
                // Occurs when clicking "Upload" without selecting any files.
                continue;
            }
            let mime_type = field.content_type().unwrap().to_string();
            let data = field
                .bytes()
                .await
                .expect("Failed to read file; the file could be too large.");
            let file = File::new(&mime_type, &filename, data);
            received_files.push(file);
        } else if name == Some("prefix") {
            let bytes = field.bytes().await.unwrap();
            prefix = String::from_utf8(bytes.to_vec()).unwrap();
        } else {
            tracing::warn!("unknown field: {:?}", name);
        }
    }

    // The prefix is shared by the batch, so filename order matches the list.
    let redirect = received_files
        .iter()
        .min_by(|a, b| a.filename.cmp(&b.filename))
        .map(|file| format!("/files#file-{}", file.sha))
        .unwrap_or_else(|| "/files".to_string());
    for file in received_files {
        let filename = if !prefix.is_empty() {
            format!("{}{}", prefix, file.filename)
        } else {
            file.filename
        };
        let file = File::new(&file.mime_type, &filename, file.data);
        File::insert(&*ctx.conn()?, &file).unwrap();
    }

    crate::trigger::trigger_github_backup(&ctx).await;
    crate::serve::see_other(&ctx, &redirect)
}

async fn get_delete(
    State(ctx): State<ServerContext>,
    Path(sha): Path<String>,
    jar: CookieJar,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    let file = File::get(&*ctx.conn()?, &sha);
    let file = match file {
        Ok(file) => file,
        Err(_) => return not_found(State(ctx.clone())).await,
    };
    let extra_head = &Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let title = format!("Delete: {}", file.filename);
    let settings = PageSettings::new(
        &title,
        Some(is_logged_in),
        None,
        false,
        Top::GoHome,
        extra_head,
    );
    let body = indoc::formatdoc! {r#"
        <div class='medium-text' style='text-align: center; font-weight: bold;'>
            <p>Are you sure you want to delete <code>{}</code>? This action cannot be undone.</p>
            <form action='/files/delete/{sha}' method='post'>
                <button type='submit'>Delete</button>
            </form>
            <br>
        </div>
    "#, file.filename};
    let body = page(&ctx, &settings, &body).await?;
    response::<String>(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

async fn post_delete(
    State(ctx): State<ServerContext>,
    Path(sha): Path<String>,
    jar: CookieJar,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    File::delete(&*ctx.conn()?, &sha).unwrap();
    tracing::info!("\"POST /files/delete/{sha} HTTP/1.1\" 303");
    crate::trigger::trigger_github_backup(&ctx).await;
    crate::serve::see_other(&ctx, "/files")
}

async fn get_rename(
    State(ctx): State<ServerContext>,
    Path(sha): Path<String>,
    jar: CookieJar,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    let file = File::get(&*ctx.conn()?, &sha);
    let file = match file {
        Ok(file) => file,
        Err(_) => return not_found(State(ctx.clone())).await,
    };
    let extra_head = &Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let title = format!("Rename: {}", file.filename);
    let settings = PageSettings::new(
        &title,
        Some(is_logged_in),
        None,
        false,
        Top::GoHome,
        extra_head,
    );
    let body = indoc::formatdoc! {r#"
        <div class='medium-text' style='text-align: center;'>
            <p>Rename file: <code>{}</code></p>
            <form action='/files/rename/{sha}' method='post'>
                <div>
                    <label for='filename'>New filename:</label>
                    <input type='text' id='filename' name='filename' value='{}' />
                </div>
                <div style='margin-top: 10px;'>
                    <button type='submit'>Change</button>
                </div>
            </form>
            <br>
        </div>
    "#, file.filename, file.filename};
    let body = page(&ctx, &settings, &body).await?;
    tracing::info!("\"GET /files/rename/{sha} HTTP/1.1\" 200");
    response::<String>(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

#[derive(Debug, Deserialize)]
struct RenameForm {
    filename: String,
}

async fn post_rename(
    State(ctx): State<ServerContext>,
    Path(sha): Path<String>,
    jar: CookieJar,
    Form(rename_form): Form<RenameForm>,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    let filename = rename_form.filename;
    File::rename(&*ctx.conn()?, &sha, &filename).unwrap();
    crate::trigger::trigger_github_backup(&ctx).await;
    crate::serve::see_other(&ctx, "/files")
}

pub fn routes(router: &Router<ServerContext>) -> Router<ServerContext> {
    router
        .clone()
        .route("/files", get(get_files))
        .route("/files/{sha}", get(get_file))
        .route("/files/{sha}/{filename}", get(get_file_with_filename))
        .route("/files/add", post(post_file))
        .route("/files/delete/{sha}", get(get_delete))
        .route("/files/delete/{sha}", post(post_delete))
        .route("/files/rename/{sha}", get(get_rename))
        .route("/files/rename/{sha}", post(post_rename))
}
