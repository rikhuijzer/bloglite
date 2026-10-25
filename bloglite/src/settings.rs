use crate::data::Kv;
use crate::data::cleanup_content;
use crate::html::PageSettings;
use crate::html::Top;
use crate::html::page;
use crate::serve::RequestResult;
use crate::serve::ServerContext;
use crate::serve::is_logged_in;
use crate::serve::response;
use axum::Form;
use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::routing::get;
use axum::routing::post;
use axum_extra::extract::CookieJar;
use rusqlite::Connection;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub site_name: String,
    pub site_description: String,
    pub author_name: String,
    pub about: String,
    pub dark_mode: Option<String>,
    pub extra_head: String,
}

impl Settings {
    pub fn from_db(conn: &Connection) -> rusqlite::Result<Self> {
        let site_name = Kv::get(conn, "site_name")?;
        let site_description = Kv::get(conn, "site_description")?;
        let author_name = Kv::get(conn, "author_name")?;
        let about = Kv::get(conn, "about")?;
        let dark_mode = Kv::get(conn, "dark_mode")?;
        let dark_mode = String::from_utf8(dark_mode).unwrap();
        let dark_mode = if dark_mode == "on" {
            Some("on".to_string())
        } else {
            None
        };
        let extra_head = Kv::get_or_empty_string(conn, "extra_head");
        Ok(Self {
            site_name: String::from_utf8(site_name).unwrap(),
            site_description: String::from_utf8(site_description).unwrap(),
            author_name: String::from_utf8(author_name).unwrap(),
            about: String::from_utf8(about).unwrap(),
            dark_mode,
            extra_head,
        })
    }
    pub fn set_about(conn: &Connection, about: &str) -> rusqlite::Result<()> {
        Kv::insert(conn, "about", about.as_bytes())?;
        Ok(())
    }
}

pub enum InputType {
    Checkbox,
    Text,
    Textarea,
}

pub fn text_input(
    input_type: InputType,
    name: &str,
    label: &str,
    value: &str,
    description: &str,
    required: bool,
) -> String {
    let required = if required { "required" } else { "" };
    let input = match input_type {
        InputType::Checkbox => {
            let value = if value == "on" { "checked" } else { "" };
            format!(
                "
            <input id='{name}' name='{name}' \
            style='margin-left: 0; \
              margin-top: 0.5rem; margin-bottom: 0.2rem;' \
            type='checkbox' {value} {required}/><br>
            "
            )
        }
        InputType::Text => format!(
            "
            <input id='{name}' name='{name}' \
            style='width: 100%; margin-left: 0; margin-right: 0; \
              margin-top: 0.5rem; margin-bottom: 0.2rem;' \
            type='text' value='{value}' {required}/><br>
            "
        ),
        InputType::Textarea => format!(
            "
            <textarea id='{name}' name='{name}' rows='7' \
            style='width: 100%; font-size: 0.8rem; \
              margin-top: 0.5rem; margin-bottom: 0.2rem;' \
            {required}>{value}</textarea><br>
            "
        ),
    };
    format!(
        "
        <label id='{name}_label' for='{name}'>{label}</label><br>
        {input}
        <span style='font-size: 0.8rem; line-height: 1.2; display: inline-block;'>
            {description}
        </span><br>
        <br>
        "
    )
}

async fn get_settings(State(ctx): State<ServerContext>, jar: CookieJar) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    let settings = match Settings::from_db(&*ctx.conn()?) {
        Ok(settings) => settings,
        Err(e) => {
            let msg = "Could not get settings from database";
            tracing::error!("{msg}: {e}");
            return crate::serve::internal_server_error(&ctx, msg).await;
        }
    };
    let style = "margin-top: 5vh; width: 100%;";
    let site_name = &settings.site_name;
    let site_name = crate::html::escape_single_quote(site_name);
    let about_description = format!(
        "This is shown below the author name on the front page. This field supports {}.",
        crate::md::markdown_link()
    );
    let extra_head_description = "
        This is added to the <code>head</code> element of the HTML page. For
        example, this can be used to modify the styling of the website via CSS.
        It can also be used to set a favicon. To do so, upload the favicon via
        files and then link to it via <br>
        <code>&lt;link rel='icon' href='/files/9f0b7bb83bd82946'&gt;</code>
        where the <code>href</code> points to the file.
    ";
    let body = format!(
        "
        <form style='{style}' \
          method='post' action='/settings'>
            {}
            {}
            {}
            {}
            {}
            {}
            <input style='margin-left: 0;' type='submit' value='Save'/>
        </form>
        ",
        text_input(
            InputType::Text,
            "site_name",
            "Site Name",
            &site_name,
            "This is shown in the title of the page.",
            true,
        ),
        text_input(
            InputType::Text,
            "site_description",
            "Site Description",
            &settings.site_description,
            "This is shown in the meta description and og:description meta tags and used by search engines to describe the website.",
            false,
        ),
        text_input(
            InputType::Text,
            "author_name",
            "Author Name",
            &settings.author_name,
            "This is shown at the homepage and in some other places.",
            true,
        ),
        text_input(
            InputType::Textarea,
            "about",
            "About",
            &settings.about,
            &about_description,
            false,
        ),
        text_input(
            InputType::Checkbox,
            "dark_mode",
            "Allow dark mode",
            if settings.dark_mode.is_some() {
                "on"
            } else {
                "off"
            },
            "When enabled, the site will allow the browser to use the dark color-scheme.",
            false,
        ),
        text_input(
            InputType::Textarea,
            "extra_head",
            "Extra HTML Head (optional)",
            &settings.extra_head,
            extra_head_description,
            false,
        ),
    );
    let page_settings =
        PageSettings::new("Settings", Some(is_logged_in), None, false, Top::GoHome, "");
    let body = page(&ctx, &page_settings, &body).await?;
    response(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

async fn post_settings(
    State(ctx): State<ServerContext>,
    jar: CookieJar,
    Form(form): Form<Settings>,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return crate::serve::unauthorized(&ctx).await;
    }
    {
        let conn = ctx.conn()?;
        Kv::insert(&conn, "site_name", form.site_name.trim().as_bytes()).unwrap();
        Kv::insert(
            &conn,
            "site_description",
            form.site_description.trim().as_bytes(),
        )
        .unwrap();
        Kv::insert(&conn, "author_name", form.author_name.trim().as_bytes()).unwrap();
        let dark_mode = if form.dark_mode.is_some() {
            "on"
        } else {
            "off"
        };
        Kv::insert(&conn, "dark_mode", dark_mode.as_bytes()).unwrap();
        let about = cleanup_content(&form.about);
        Kv::insert(&conn, "about", about.as_bytes()).unwrap();
        let extra_head = cleanup_content(&form.extra_head);
        Kv::insert(&conn, "extra_head", extra_head.as_bytes()).unwrap();
    }
    crate::trigger::trigger_github_backup(&ctx).await;
    crate::serve::see_other(&ctx, "/")
}

pub fn routes(router: &Router<ServerContext>) -> Router<ServerContext> {
    router
        .clone()
        .route("/settings", get(get_settings))
        .route("/settings", post(post_settings))
}
