//! This module contains all UI-related logic, including Axum handlers and Maud templates.

use crate::{d1_storage, state::strategy::ApiKey, util, AppState};
use axum::{
    extract::{Form, FromRef, FromRequestParts, Path, Query, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Json, Redirect, Response},
    routing::get,
    Router,
};
use maud::{html, Markup, PreEscaped, DOCTYPE};
use phf::phf_map;
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::sync::Arc;
use time::Duration;
use tower_cookies::{Cookie, Cookies};

// --- Constants for Providers ---

struct ProviderConfig {
    color: &'static str,
    icon: &'static str,
    bg_color: &'static str,
}

static PROVIDER_CONFIGS: phf::Map<&'static str, ProviderConfig> = phf_map! {
    "google-ai-studio" => ProviderConfig { color: "from-red-400 to-yellow-400", icon: "G", bg_color: "from-red-50 to-yellow-50" },
    "google-vertex-ai" => ProviderConfig { color: "from-blue-400 to-green-400", icon: "▲", bg_color: "from-blue-50 to-green-50" },
    "anthropic" => ProviderConfig { color: "from-orange-400 to-red-400", icon: "A", bg_color: "from-orange-50 to-red-50" },
    "azure-openai" => ProviderConfig { color: "from-blue-500 to-cyan-400", icon: "⊞", bg_color: "from-blue-50 to-cyan-50" },
    "aws-bedrock" => ProviderConfig { color: "from-yellow-500 to-orange-500", icon: "◆", bg_color: "from-yellow-50 to-orange-50" },
    "cartesia" => ProviderConfig { color: "from-purple-400 to-pink-400", icon: "C", bg_color: "from-purple-50 to-pink-50" },
    "cerebras-ai" => ProviderConfig { color: "from-gray-600 to-gray-800", icon: "◉", bg_color: "from-gray-50 to-gray-100" },
    "cohere" => ProviderConfig { color: "from-green-400 to-teal-500", icon: "●", bg_color: "from-green-50 to-teal-50" },
    "deepseek" => ProviderConfig { color: "from-indigo-500 to-purple-600", icon: "◈", bg_color: "from-indigo-50 to-purple-50" },
    "elevenlabs" => ProviderConfig { color: "from-pink-400 to-rose-500", icon: "♫", bg_color: "from-pink-50 to-rose-50" },
    "grok" => ProviderConfig { color: "from-gray-700 to-black", icon: "X", bg_color: "from-gray-50 to-gray-100" },
    "groq" => ProviderConfig { color: "from-orange-500 to-red-600", icon: "⚡", bg_color: "from-orange-50 to-red-50" },
    "huggingface" => ProviderConfig { color: "from-yellow-400 to-amber-500", icon: "🤗", bg_color: "from-yellow-50 to-amber-50" },
    "mistral" => ProviderConfig { color: "from-blue-600 to-indigo-700", icon: "M", bg_color: "from-blue-50 to-indigo-50" },
    "openai" => ProviderConfig { color: "from-emerald-400 to-teal-600", icon: "◯", bg_color: "from-emerald-50 to-teal-50" },
    "openrouter" => ProviderConfig { color: "from-violet-500 to-purple-600", icon: "⟲", bg_color: "from-violet-50 to-purple-50" },
    "perplexity-ai" => ProviderConfig { color: "from-cyan-500 to-blue-600", icon: "?", bg_color: "from-cyan-50 to-blue-50" },
    "replicate" => ProviderConfig { color: "from-slate-500 to-gray-600", icon: "⧉", bg_color: "from-slate-50 to-gray-50" },
};

// --- Router ---

pub fn ui_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(get_providers_page_handler))
        .route(
            "/login",
            get(get_login_page_handler).post(post_login_handler),
        )
        .route(
            "/keys/{provider}",
            get(get_keys_list_page_handler).post(post_keys_list_handler),
        )
        .route("/api/keys/{id}/coolings", get(get_key_coolings_handler))
}

// --- Handlers ---

// region: --- Login Handlers
#[derive(Deserialize)]
pub struct LoginForm {
    auth_key: String,
}

pub async fn get_login_page_handler() -> Markup {
    page_layout(login_page())
}

pub async fn post_login_handler(
    cookies: Cookies,
    State(state): State<Arc<AppState>>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    if util::is_valid_auth_key(&form.auth_key, &state.env) {
        let cookie = Cookie::build(("auth_key", form.auth_key))
            .path("/")
            .http_only(true)
            .same_site(tower_cookies::cookie::SameSite::Strict)
            .max_age(Duration::days(365));
        cookies.add(cookie.into());
        Redirect::to("/").into_response()
    } else {
        (StatusCode::FORBIDDEN, "Invalid auth key").into_response()
    }
}
// endregion: --- Login Handlers

// region: --- Provider Page Handlers
pub async fn get_providers_page_handler(_layout: PageLayout) -> Markup {
    page_layout(providers_page())
}
// endregion: --- Provider Page Handlers

// region: --- Keys List Page Handlers
#[derive(Deserialize, Default, Debug)]
pub struct KeysListParams {
    q: Option<String>,
    status: Option<String>,
    page: Option<usize>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

// #[axum::debug_handler]
#[worker::send]
pub async fn get_keys_list_page_handler(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Query(params): Query<KeysListParams>,
    _layout: PageLayout,
) -> Response {
    let status: &str = params.status.as_deref().unwrap_or("active");
    let q: &str = params.q.as_deref().unwrap_or("");
    let page = params.page.unwrap_or(1);
    let sort_by: &str = params.sort_by.as_deref().unwrap_or("");
    let sort_order: &str = params.sort_order.as_deref().unwrap_or("desc");
    let db = match state.env.d1("DB") {
        Ok(db) => db,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
                .into_response()
        }
    };

    let (keys, total) =
        // match d1_storage::list_keys(&db, &provider, status, q, page, 20, sort_by, sort_order).await
        match d1_storage::list_keys(&db, provider.as_str(), &status, &q, page, 20, sort_by, sort_order).await
        {
            Ok(data) => data,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to list keys: {}", e),
                )
                    .into_response()
            }
        };

    let content = keys_list_page(
        // &provider, status, q, keys, total, page, 20, sort_by, sort_order,
        provider.as_str(),
        status,
        q,
        keys,
        total,
        page,
        20,
        sort_by,
        sort_order,
    );
    //(
    //    StatusCode::OK,
    //    format!(
    //        "Provider: {}, Status: {}, Q: {}, Page: {}",
    //        provider, status, q, page
    //    ),
    //)
    // .into_response()
    (StatusCode::OK, page_layout(content)).into_response()
}

// When a form has multiple checkboxes with the same name, it can be submitted
// as either a sequence of values (if multiple are checked) or a single string
// (if only one is checked). This custom deserializer handles both cases and
// always returns a Vec<String>.
fn deserialize_one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OneOrManyVisitor;

    impl<'de> serde::de::Visitor<'de> for OneOrManyVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or a sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(vec![value.to_owned()])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(item) = seq.next_element()? {
                vec.push(item);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(OneOrManyVisitor)
}

#[derive(Deserialize, Debug)]
pub struct KeysListForm {
    action: String,
    keys: Option<String>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    key_id: Vec<String>,
}

// #[axum::debug_handler]
// pub async fn get_keys_list_page_handler(
//     State(state): State<AppState>,
//     Path(provider): Path<String>,
//     Query(params): Query<KeysListParams>,
// ) -> impl IntoResponse {
//     // Your handler implementation
//     // (StatusCode::OK, "Handler working!")
// }

//#[axum::debug_handler]
//pub async fn get_keys_list_page_handler(
//    State(state): State<AppState>,
//    Path(provider): Path<String>,
//    Query(params): Query<KeysListParams>,
//) -> impl IntoResponse {
//    let status = params.status.as_deref().unwrap_or("active");
//    let q = params.q.as_deref().unwrap_or("");
//    let page = params.page.unwrap_or(1);
//    let sort_by = params.sort_by.as_deref().unwrap_or("");
//    let sort_order = params.sort_order.as_deref().unwrap_or("desc");
//
//    let db = match state.env.d1("DB") {
//        Ok(db) => db,
//        Err(e) => {
//            return (
//                StatusCode::INTERNAL_SERVER_ERROR,
//                format!("Database error: {}", e),
//            )
//                .into_response()
//        }
//    };
//
//    let (keys, total) =
//        match d1_storage::list_keys(&db, &provider, status, q, page, 20, sort_by, sort_order).await
//        {
//            Ok(data) => data,
//            Err(e) => {
//                return (
//                    StatusCode::INTERNAL_SERVER_ERROR,
//                    format!("Failed to list keys: {}", e),
//                )
//                    .into_response()
//            }
//        };
//
//    let content = keys_list_page(
//        &provider, status, q, keys, total, page, 20, // pageSize
//        sort_by, sort_order,
//    );
//
//    (StatusCode::OK, page_layout(content)).into_response()
//}

#[worker::send]
pub async fn post_keys_list_handler(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Form(form): Form<KeysListForm>,
) -> impl IntoResponse {
    if form.action == "add" {
        if let Some(keys_str) = form.keys {
            let db = state.env.d1("DB").unwrap();
            match d1_storage::add_keys(&db, &provider, &keys_str).await {
                Ok(_) => (), // All good
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to add keys: {}", e),
                    )
                        .into_response()
                }
            }
        }
    } else if form.action == "delete" {
        if !form.key_id.is_empty() {
            let db = state.env.d1("DB").unwrap();
            match d1_storage::delete_keys(&db, form.key_id).await {
                Ok(_) => (), // All good
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to delete keys: {}", e),
                    )
                        .into_response()
                }
            }
        }
    } else if form.action == "delete-all-blocked" {
        let db = state.env.d1("DB").unwrap();
        match d1_storage::delete_all_blocked(&db, &provider).await {
            Ok(_) => (), // All good
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to delete all blocked keys: {}", e),
                )
                    .into_response()
            }
        }
    }

    // Redirect back to the keys list page
    Redirect::to(&format!("/keys/{}", provider)).into_response()
}

//#[axum::debug_handler]
//pub async fn get_keys_list_page_handler(
//    _layout: PageLayout,
//    State(state): State<AppState>,
//    Path(provider): Path<String>,
//    Query(params): Query<KeysListParams>,
//) -> impl IntoResponse {
//    let status = params.status.as_deref().unwrap_or("active");
//    let q = params.q.as_deref().unwrap_or("");
//    let page = params.page.unwrap_or(1);
//    let sort_by = params.sort_by.as_deref().unwrap_or("");
//    let sort_order = params.sort_order.as_deref().unwrap_or("desc");
//
//    let db = match state.env.d1("DB") {
//        Ok(db) => db,
//        Err(e) => {
//            return (
//                StatusCode::INTERNAL_SERVER_ERROR,
//                format!("Database error: {}", e),
//            )
//                .into_response()
//        }
//    };
//
//    let (keys, total) =
//        match d1_storage::list_keys(&db, &provider, status, q, page, 20, sort_by, sort_order).await
//        {
//            Ok(data) => data,
//            Err(e) => {
//                return (
//                    StatusCode::INTERNAL_SERVER_ERROR,
//                    format!("Failed to list keys: {}", e),
//                )
//                    .into_response()
//            }
//        };
//
//    let content = keys_list_page(
//        &provider, status, q, keys, total, page, 20, // pageSize
//        sort_by, sort_order,
//    );
//    (StatusCode::OK, content).into_response()
//}
// endregion: --- Keys List Page Handlers

// region: --- API Handlers
#[worker::send]
pub async fn get_key_coolings_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    _layout: PageLayout,
) -> Response {
    let db = match state.env.d1("DB") {
        Ok(db) => db,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
                .into_response()
        }
    };

    match d1_storage::get_key_coolings(&db, &id).await {
        Ok(Some(key)) => (StatusCode::OK, Json(key)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Key not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get key coolings: {}", e),
        )
            .into_response(),
    }
}
// endregion: --- API Handlers

// --- Page Components (Maud HTML) ---

// region: --- Layout
fn page_layout(content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "One Balance" }
                link rel="icon" href="data:image/svg+xml,<svg xmlns=%22http://www.w3.org/2000/svg%22 viewBox=%220 0 100 100%22><text y=%22.9em%22 font-size=%2290%22>⚖️</text></svg>";
                script src="https://cdn.tailwindcss.com" {}
                style { (PreEscaped(include_str!("web/style.css"))) }
                script { (PreEscaped(include_str!("web/script.js"))) }
            }
            body class="breathing-bg min-h-screen text-gray-900 flex flex-col" {
                main class="container mx-auto mt-12 px-6 max-w-7xl flex-grow" {
                    (content)
                }
                footer class="text-center py-12 text-sm text-gray-600 space-y-3" {
                    p {
                        a href="https://github.com/inevity/theone" target="_blank" rel="noopener noreferrer" class="hover:text-blue-600 transition-colors duration-300 font-medium" {
                            "THEONE on GitHub"
                        }
                    }
                    p {
                        a href="https://github.com/glidea/zenfeed" target="_blank" rel="noopener noreferrer" class="hover:text-blue-600 transition-colors duration-300 font-medium" {
                            "zenfeed — Make RSS 📰 great again with AI 🧠✨!!"
                        }
                    }
                }
            }
        }
    }
}
// endregion: --- Layout

// region: --- Login Page
fn login_page() -> Markup {
    html! {
        div class="flex items-center justify-center min-h-[70vh] relative" {
            div class="absolute top-20 left-1/4 w-32 h-32 bg-blue-200/30 rounded-full blur-3xl floating-element" {}
            div class="absolute bottom-20 right-1/4 w-40 h-40 bg-amber-200/30 rounded-full blur-3xl floating-element" style="animation-delay: -3s;" {}

            div class="max-w-md w-full mx-6 relative z-10" {
                div class="text-center mb-16" {
                    div class="pulse-glow inline-block p-6 bg-gradient-to-br from-blue-100 to-indigo-100 rounded-3xl mb-6" {
                        h1 class="text-6xl font-bold" { "⚖️" }
                    }
                    h2 class="text-4xl font-bold bg-gradient-to-r from-gray-900 to-gray-700 bg-clip-text text-transparent mb-3" { "One Balance" }
                    p class="text-gray-600 text-lg" { "Manage your API keys with perfect balance" }
                }

                div class="glass-card-warm rounded-3xl p-10 transition-all duration-500 hover:scale-[1.02]" {
                    form action="/login" method="POST" class="space-y-8" {
                        div {
                            label for="auth_key" class="block text-gray-800 text-sm font-bold mb-4 tracking-wide" { "Authentication Key" }
                            input type="password" id="auth_key" name="auth_key"
                                   class="input-field w-full px-5 py-4 rounded-2xl text-gray-900 placeholder-gray-500 focus:outline-none text-base font-medium"
                                   placeholder="Enter your auth key" required;
                        }
                        button type="submit" class="btn-primary w-full py-4 px-6 text-white font-bold rounded-2xl focus:outline-none focus:ring-4 focus:ring-blue-200 text-base tracking-wide" {
                            "Sign In"
                        }
                    }
                }
            }
        }
    }
}
// endregion: --- Login Page

// region: --- Providers Page
fn providers_page() -> Markup {
    html! {
        div class="text-center mb-20 relative" {
            div class="absolute top-0 left-1/2 transform -translate-x-1/2 -translate-y-8 w-64 h-32 bg-gradient-to-r from-blue-200/20 to-purple-200/20 rounded-full blur-3xl" {}
            h1 class="text-6xl font-bold bg-gradient-to-r from-gray-900 via-blue-800 to-gray-900 bg-clip-text text-transparent mb-6 relative" { "Select Provider" }
        }

        div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-8 max-w-7xl mx-auto" {
            @for (p_name, config) in &PROVIDER_CONFIGS {
                div class="glass-card rounded-3xl p-8 transition-all duration-500 hover:cursor-pointer group hover:shadow-2xl" {
                    a href={"/keys/" (p_name) "?status=active"} class="block" {
                        div class="flex items-center justify-between" {
                            div class="flex items-center space-x-5" {
                                div class="relative" {
                                    div class={"w-14 h-14 bg-gradient-to-br "(config.bg_color)" rounded-2xl flex items-center justify-center group-hover:scale-110 transition-all duration-300 shadow-lg"} {
                                        div class={"w-8 h-8 bg-gradient-to-br "(config.color)" rounded-xl flex items-center justify-center text-white font-bold text-sm shadow-inner"} {
                                            (config.icon)
                                        }
                                    }
                                    div class={"absolute -top-1 -right-1 w-4 h-4 bg-gradient-to-br "(config.color)" rounded-full opacity-60 group-hover:opacity-100 transition-opacity duration-300"} {}
                                }
                                div {
                                    h3 class="text-xl font-bold text-gray-900 group-hover:text-blue-600 transition-colors duration-300 mb-1" { (p_name) }
                                }
                            }
                            div class="flex items-center space-x-2" {
                                div class={"w-2 h-2 bg-gradient-to-r "(config.color)" rounded-full opacity-60 group-hover:opacity-100 transition-opacity duration-300"} {}
                                svg class="w-6 h-6 text-gray-400 transform transition-all duration-300 group-hover:translate-x-2 group-hover:text-blue-500" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                                    path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
// endregion: --- Providers Page

// region: --- Keys List Page
fn keys_list_page(
    provider: &str,
    current_status: &str,
    q: &str,
    keys: Vec<ApiKey>,
    total: i32,
    page: usize,
    page_size: usize,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    html! {
        (build_breadcrumb(provider))
        (build_keys_table(provider, current_status, q, keys, total, page, page_size, sort_by, sort_order))
        (build_add_keys_form(provider, current_status, q, page, sort_by, sort_order))
        (build_model_coolings_modal())
    }
}

fn build_breadcrumb(provider: &str) -> Markup {
    html! {
        div class="mb-8" {
            nav class="flex items-center space-x-2 text-sm text-gray-600 mb-4" {
                a href="/" class="hover:text-blue-600 transition-colors duration-200 font-medium" { "Providers" }
                svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                    path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" {}
                }
                span class="text-gray-900 font-semibold" { (provider) }
            }
        }
    }
}

fn build_keys_table(
    provider: &str,
    current_status: &str,
    q: &str,
    keys: Vec<ApiKey>,
    total: i32,
    page: usize,
    page_size: usize,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    let key_rows = build_key_rows(keys);
    let pagination_controls = build_pagination_controls(
        provider,
        current_status,
        q,
        page,
        page_size,
        total as usize,
        sort_by,
        sort_order,
    );

    html! {
        div class="glass-card bg-white/80 rounded-3xl shadow-xl border border-gray-200 overflow-hidden mb-8 max-w-5xl mx-auto backdrop-blur-xl" {
            form method="POST" {
                (build_table_header(provider, current_status, q, sort_by, sort_order))
                (build_table_content(&key_rows, provider, current_status, q, sort_by, sort_order))
                (build_table_footer(total, &pagination_controls))
            }
            (build_search_form(provider, current_status))
        }
    }
}

fn build_table_header(
    provider: &str,
    current_status: &str,
    q: &str,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    let status_tabs = build_status_tabs(provider, current_status, q, sort_by, sort_order);
    let delete_all_button = if current_status == "blocked" {
        html! {
            button type="submit" name="action" value="delete-all-blocked"
                    onclick="return confirm('Are you sure you want to delete all blocked keys? This action cannot be undone.');"
                    class="px-4 py-2.5 bg-red-800 hover:bg-red-900 text-white font-semibold rounded-xl text-sm transition-all duration-200 hover:shadow-lg hover:shadow-red-800/25 hover:-translate-y-0.5 border border-red-800" {
                "Delete ALL"
            }
        }
    } else {
        html! {}
    };

    html! {
        div class="p-6 border-b border-gray-200/60 bg-white/30 backdrop-blur-sm" {
            div class="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-4" {
                div class="flex flex-col sm:flex-row items-start sm:items-center gap-4" {
                    div class="flex gap-2" { (status_tabs) }
                    div class="flex items-center" {
                        div class="relative" {
                            svg class="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                                path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" {}
                            }
                            input form="search-form" type="search" name="q" value=(q)
                                   placeholder="Search keys..."
                                   class="input-field w-64 pl-10 pr-4 py-2.5 bg-white border border-gray-300 rounded-xl text-gray-900 placeholder-gray-500 focus:outline-none text-sm shadow-sm";
                        }
                    }
                }
                div class="flex items-center gap-2" {
                    button type="submit" name="action" value="delete"
                            class="px-4 py-2.5 bg-red-600 hover:bg-red-700 text-white font-semibold rounded-xl text-sm transition-all duration-200 hover:shadow-lg hover:shadow-red-600/25 hover:-translate-y-0.5 border border-red-600" {
                        "Delete Selected"
                    }
                    (delete_all_button)
                }
            }
        }
    }
}

fn build_status_tabs(
    provider: &str,
    current_status: &str,
    q: &str,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    let statuses = ["active", "blocked"];
    html! {
        @for s in &statuses {
            @let is_active = *s == current_status;
            @let active_classes = if is_active {
                "bg-blue-600 text-white shadow-lg shadow-blue-600/30 border border-blue-600"
            } else {
                "bg-white/80 text-gray-800 hover:bg-white border border-gray-300 hover:border-gray-400"
            };
            @let link = build_page_link(provider, s, q, 1, 20, sort_by, sort_order);
            a href=(link) class={"px-6 py-2.5 rounded-xl text-sm font-semibold transition-all duration-200 " (active_classes)} { (s.chars().next().unwrap().to_uppercase().to_string() + &s[1..]) }
        }
    }
}

fn build_search_form(provider: &str, current_status: &str) -> Markup {
    html! {
        form id="search-form" method="GET" action={"/keys/" (provider)} class="hidden" {
            input type="hidden" name="status" value=(current_status);
        }
    }
}

fn build_table_content(
    key_rows: &Markup,
    provider: &str,
    current_status: &str,
    q: &str,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    html! {
        div class="overflow-x-auto" {
            table class="w-full table-fixed" {
                colgroup {
                    col class="w-12";
                    col class="w-80";
                    col class="w-32";
                    col class="w-24";
                }
                thead {
                    tr class="bg-gradient-to-r from-slate-100/90 to-gray-100/90 border-b border-gray-400/80 backdrop-blur-sm" {
                        th class="p-4 text-left" {
                            input type="checkbox"
                                   onchange="document.querySelectorAll('[name=key_id]').forEach(c => c.checked = this.checked)"
                                   class="h-4 w-4 text-blue-600 bg-white border-gray-500 rounded focus:ring-blue-500 transition-colors backdrop-blur-sm";
                        }
                        th class="p-4 text-left font-semibold text-slate-800 text-sm tracking-wide" { "API Key" }
                        (sortable_th("Cooling Time", "totalCoolingSeconds", provider, current_status, q, sort_by, sort_order))
                        (sortable_th("Used Time", "createdAt", provider, current_status, q, sort_by, sort_order))
                    }
                }
                tbody class="divide-y divide-gray-300/60" {
                    (key_rows)
                }
            }
        }
    }
}

fn build_key_rows(keys: Vec<ApiKey>) -> Markup {
    if keys.is_empty() {
        return build_empty_state();
    }
    html! {
        @for k in keys {
            tr class="group hover:bg-blue-100/60 even:bg-slate-100/40 odd:bg-white/60 transition-all duration-300 hover:shadow-md backdrop-blur-sm border-b border-gray-300/50" {
                td class="p-4" {
                    input type="checkbox" name="key_id" value=(k.id)
                           class="h-4 w-4 text-blue-600 bg-white border-gray-500 rounded focus:ring-blue-500 focus:ring-2 transition-colors backdrop-blur-sm";
                }
                td class="p-4" {
                    (build_copyable_key(&k.key))
                }
                td class="p-4" {
                    span class="text-sm text-slate-800 cursor-pointer hover:text-blue-700 transition-colors duration-200 font-medium px-2 py-1 rounded-md hover:bg-blue-100/80 backdrop-blur-sm"
                          title="Click to view model cooling details"
                          onclick=(format!("showModelCoolings('{}', '{}')", k.id, k.key)) { (format_cooling_time(k.total_cooling_seconds)) }
                }
                td class="p-4 text-sm text-slate-700 font-medium" { (format_used_time(k.created_at)) }
            }
        }
    }
}

fn sortable_th(
    title: &str,
    sort_key: &str,
    provider: &str,
    status: &str,
    q: &str,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    let (new_sort_order, icon) = if sort_by == sort_key {
        if sort_order == "asc" {
            ("desc", "▲")
        } else {
            ("asc", "▼")
        }
    } else {
        ("desc", "")
    };

    let link = build_page_link(provider, status, q, 1, 20, sort_key, new_sort_order);

    html! {
        th class="p-4 text-left font-semibold text-slate-800 text-sm tracking-wide" {
            a href=(link) class="flex items-center gap-1 hover:text-blue-600 transition-colors" {
                (title)
                @if !icon.is_empty() {
                    span class="text-blue-600" { (icon) }
                }
            }
        }
    }
}

fn build_copyable_key(key: &str) -> Markup {
    html! {
        div class="relative inline-block" {
            code class="px-3 py-2 bg-slate-200/80 border border-slate-300/70 rounded-lg text-sm font-mono text-slate-900 cursor-pointer hover:bg-slate-300/80 hover:border-slate-400/70 transition-all duration-200 inline-block truncate max-w-full group-hover:shadow-sm backdrop-blur-sm"
                  onclick=(format!("copyToClipboard('{}', this)", key))
                  title="Click to copy" { (key) }
            div class="absolute -top-8 left-1/2 transform -translate-x-1/2 bg-emerald-700 text-white text-xs px-2 py-1 rounded opacity-0 pointer-events-none transition-opacity duration-300 whitespace-nowrap copy-tooltip backdrop-blur-sm" {
                "Copied!"
            }
        }
    }
}

fn format_used_time(created_at: u64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as u64;
    let used_seconds = now.saturating_sub(created_at);
    let days = used_seconds / 86400;
    let hours = (used_seconds % 86400) / 3600;
    let minutes = (used_seconds % 3600) / 60;

    if days > 0 {
        format!("{}d{}h", days, hours)
    } else if hours > 0 {
        format!("{}h{}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

fn format_cooling_time(total_seconds: u64) -> String {
    if total_seconds == 0 {
        return "-".to_string();
    }
    let days = total_seconds / 86400;
    let hours = (total_seconds % 86400) / 3600;
    let minutes = (total_seconds % 3600) / 60;

    if days > 0 {
        format!("{}d{}h", days, hours)
    } else if hours > 0 {
        format!("{}h{}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

fn build_empty_state() -> Markup {
    html! {
        tr {
            td colspan="4" class="text-center p-12 text-gray-700 bg-slate-100/40 backdrop-blur-sm" {
                div class="flex flex-col items-center gap-3" {
                    svg class="w-12 h-12 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                        path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4" {}
                    }
                    p class="font-medium" { "No keys found" }
                }
            }
        }
    }
}

fn build_table_footer(total: i32, pagination_controls: &Markup) -> Markup {
    if total == 0 {
        return html! {};
    }

    html! {
        div class="flex justify-center items-center p-6 border-t border-gray-300/80 bg-gray-100/60 backdrop-blur-sm" {
            div class="flex items-center gap-2 p-3 bg-white/90 rounded-xl border border-gray-300/80 shadow-sm backdrop-blur-sm" {
                (pagination_controls)
                div class="h-6 w-px bg-gray-300/80" {}
                div class="px-3 text-gray-600 text-sm font-semibold" {
                    (total)
                }
            }
        }
    }
}

fn build_pagination_controls(
    provider: &str,
    current_status: &str,
    q: &str,
    page: usize,
    page_size: usize,
    total: usize,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    let num_pages = (total as f64 / page_size as f64).ceil() as usize;
    if num_pages <= 1 {
        return html! {};
    }

    let page_numbers = generate_page_numbers(page, num_pages);
    let prev_page = page.saturating_sub(1);
    let next_page = (page + 1).min(num_pages);
    let prev_disabled = page <= 1;
    let next_disabled = page >= num_pages;

    html! {
        (build_pagination_button("prev", prev_page, prev_disabled, provider, current_status, q, sort_by, sort_order))
        @for p in page_numbers {
            @if let Some(page_num) = p {
                (build_page_number_button(page_num, page, provider, current_status, q, sort_by, sort_order))
            } @else {
                span class="px-3 py-2 text-sm font-medium text-gray-500" { "..." }
            }
        }
        (build_pagination_button("next", next_page, next_disabled, provider, current_status, q, sort_by, sort_order))
    }
}

fn generate_page_numbers(current_page: usize, total_pages: usize) -> Vec<Option<usize>> {
    let mut pages = vec![];
    let window = 2;

    // Use a simpler logic for fewer pages
    if total_pages <= (2 * window + 3) {
        for i in 1..=total_pages {
            pages.push(Some(i));
        }
    } else {
        pages.push(Some(1));
        if current_page > window + 2 {
            pages.push(None); // Ellipsis
        }

        let start = (current_page.saturating_sub(window)).max(2);
        let end = (current_page + window).min(total_pages - 1);

        for i in start..=end {
            pages.push(Some(i));
        }

        if current_page < total_pages.saturating_sub(window + 1) {
            pages.push(None); // Ellipsis
        }
        pages.push(Some(total_pages));
    }
    pages
}

fn build_pagination_button(
    btn_type: &str,
    target_page: usize,
    disabled: bool,
    provider: &str,
    status: &str,
    q: &str,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    let icon = if btn_type == "prev" {
        html! { path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" {} }
    } else {
        html! { path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" {} }
    };

    let link = build_page_link(provider, status, q, target_page, 20, sort_by, sort_order);
    let base_classes = "p-2 rounded-lg text-sm font-medium transition-all duration-200";
    let disabled_classes =
        "bg-gray-200 text-gray-400 cursor-not-allowed border border-gray-300 pointer-events-none";
    let enabled_classes = "bg-white text-gray-800 hover:bg-gray-50 border border-gray-300 hover:border-gray-400 shadow-sm";

    //html! {
    //    a href=[if disabled { "#" } else { &link }]
    //       class=[base_classes, if disabled { disabled_classes } else { enabled_classes }] {
    //        svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" { (icon) }
    //    }
    //}
    let href_value = if disabled { "#" } else { &link };
    let class_value = format!(
        "{} {}",
        base_classes,
        if disabled {
            disabled_classes
        } else {
            enabled_classes
        }
    );

    html! {
        a href=(href_value) class=(class_value) {
            svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" { (icon) }
        }
    }
}

fn build_page_number_button(
    page_item: usize,
    current_page: usize,
    provider: &str,
    status: &str,
    q: &str,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    let is_current = page_item == current_page;
    let link = build_page_link(provider, status, q, page_item, 20, sort_by, sort_order);
    let base_classes = "px-3 py-2 rounded-lg text-sm font-medium transition-all duration-200";
    let current_classes = "bg-blue-600 text-white shadow-lg shadow-blue-600/30 border border-blue-600 pointer-events-none";
    let other_classes = "bg-white text-gray-800 hover:bg-gray-50 border border-gray-300 hover:border-gray-400 shadow-sm";

    //html! {
    //    a href=[if is_current { "#" } else { &link }]
    //       class=[base_classes, if is_current { current_classes } else { other_classes }] {
    //        (page_item)
    //    }
    //}

    let href_value = if is_current { "#" } else { &link };
    let class_value = format!(
        "{} {}",
        base_classes,
        if is_current {
            current_classes
        } else {
            other_classes
        }
    );

    html! {
        a href=(href_value) class=(class_value) {
            (page_item)
        }
    }
}

fn build_page_link(
    provider: &str,
    status: &str,
    q: &str,
    page: usize,
    _page_size: usize,
    sort_by: &str,
    sort_order: &str,
) -> String {
    let mut params = vec![];
    if !status.is_empty() {
        params.push(format!("status={}", status));
    }
    if !q.is_empty() {
        params.push(format!("q={}", q));
    }
    if !sort_by.is_empty() {
        params.push(format!("sort_by={}", sort_by));
        params.push(format!("sort_order={}", sort_order));
    }
    if page > 1 {
        params.push(format!("page={}", page));
    }
    format!("/keys/{}?{}", provider, params.join("&"))
}

// endregion: --- Keys List Page

fn build_add_keys_form(
    provider: &str,
    current_status: &str,
    q: &str,
    page: usize,
    sort_by: &str,
    sort_order: &str,
) -> Markup {
    html! {
        div class="glass-card bg-white/80 rounded-3xl shadow-xl p-6 border border-gray-200 max-w-5xl mx-auto" {
            div class="flex items-center gap-3 mb-6" {
                div class="p-2 bg-blue-100 rounded-xl border border-blue-200" {
                    svg class="w-5 h-5 text-blue-700" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                        path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" {}
                    }
                }
                h2 class="text-xl font-bold text-gray-900" { "Add New Keys" }
            }
            form method="POST" {
                input type="hidden" name="action" value="add";
                div class="mb-6" {
                    label class="block text-gray-800 text-sm font-semibold mb-3" { "API Keys" }
                    textarea name="keys"
                              class="input-field w-full p-4 bg-white border border-gray-300 rounded-xl text-gray-900 placeholder-gray-500 focus:outline-none font-mono text-sm resize-none shadow-sm"
                              rows="4"
                              placeholder="Enter API keys, one per line or separated by commas" {}
                }
                div class="flex justify-end" {
                    button type="submit"
                            formaction={"/keys/" (provider)}
                            class="btn-primary px-6 py-3 text-white font-semibold rounded-xl focus:outline-none focus:ring-4 focus:ring-blue-200" {
                        "Add Keys"
                    }
                }
            }
        }
    }
}

fn build_model_coolings_modal() -> Markup {
    html! {
        div id="modelCoolingsModal" class="fixed inset-0 bg-black bg-opacity-50 backdrop-blur-sm hidden items-center justify-center z-50" onclick="closeModal(event)" {
            div class="glass-card bg-white rounded-3xl shadow-2xl border border-gray-200 max-w-2xl w-full mx-6 max-h-[80vh] overflow-hidden" onclick="event.stopPropagation()" {
                div class="p-6 border-b border-gray-200 bg-white/80" {
                    div class="flex items-center justify-between" {
                        h3 class="text-xl font-bold text-gray-900" { "Model Cooling Details" }
                        button onclick="closeModal()" class="p-2 hover:bg-gray-100 rounded-lg transition-colors duration-200" {
                            svg class="w-5 h-5 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24" {
                                path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" {}
                            }
                        }
                    }
                    p class="text-sm text-gray-600 mt-2" { "Key: " span id="modalKeyName" class="font-mono" {} }
                }
                div class="p-6 overflow-y-auto max-h-96" {
                    div id="modelCoolingsTable" {}
                }
            }
        }
    }
}

/*
// --- Authentication & Error Handling ---

// region: --- WebError
#[derive(Debug)]
pub enum WebError {
    Worker(worker::Error),
    Auth,
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        match self {
            WebError::Worker(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Worker Error: {}", e),
            )
                .into_response(),
            WebError::Auth => Redirect::to("/login").into_response(),
        }
    }
}

impl From<worker::Error> for WebError {
    fn from(e: worker::Error) -> Self {
        WebError::Worker(e)
    }
}
// endregion: --- WebError
*/

// region: --- PageLayout Extractor
pub struct PageLayout;

impl<S> FromRequestParts<S> for PageLayout
where
    S: Send + Sync,
    Arc<AppState>: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = Arc::<AppState>::from_ref(state);
        let cookies = Cookies::from_request_parts(parts, state)
            .await
            .map_err(|rejection| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Cookie error: {} {}", rejection.0, rejection.1),
                )
                    .into_response()
            })?;

        if let Some(cookie) = cookies.get("auth_key") {
            let auth_key = cookie.value().to_string();
            if util::is_valid_auth_key(&auth_key, &app_state.env) {
                return Ok(PageLayout);
            }
        }

        Err(Redirect::to("/login").into_response())
    }
}
//impl<S, B> FromRequest<S, B> for PageLayout
//where
//    B: Send,
//    S: Send + Sync,
//    AppState: FromRef<S>,
//{
//    type Rejection = Response;
//
//    async fn from_request(req: RequestParts<S, B>) -> Result<Self, Self::Rejection> {
//        // Delegate to your FromRequestParts impl to extract from parts
//        Self::from_request_parts(req.parts(), req.extensions()).await
//    }
//}

//use std::{future::Future, pin::Pin};
//impl<S> FromRequestParts<S> for PageLayout
//where
//    S: Send + Sync + 'static,
//    AppState: FromRef<S>,
//{
//    type Rejection = Response;
//
//    fn from_request_parts(
//        parts: &mut Parts,
//        state: &S,
//    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Rejection>> + Send>> {
//        let state = AppState::from_ref(state);
//        let mut parts = std::mem::take(parts); // move out to avoid borrow issues
//
//        Box::pin(async move {
//            let cookies = Cookies::from_request_parts(&mut parts, &state)
//                .await
//                .map_err(|rejection| {
//                    (
//                        StatusCode::INTERNAL_SERVER_ERROR,
//                        format!("Cookie error: {} {}", rejection.0, rejection.1),
//                    )
//                        .into_response()
//                })?;
//
//            if let Some(cookie) = cookies.get("auth_key") {
//                let auth_key = cookie.value().to_string();
//                if util::is_valid_auth_key(&auth_key, &state.env) {
//                    return Ok(PageLayout);
//                }
//            }
//
//            Err(Redirect::to("/login").into_response())
//        })
//    }
//}
//impl<S> FromRequestParts<S> for PageLayout
//where
//    S: Send + Sync,
//    AppState: FromRef<S>,
//{
//    type Rejection = (StatusCode, &'static str); // Use standard rejection
//
//    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
//        let app_state = AppState::from_ref(state);
//        let cookies = Cookies::from_request_parts(parts, state)
//            .await
//            .map_err(|_| (StatusCode::BAD_REQUEST, "Cookie error"))?;
//
//        if let Some(cookie) = cookies.get("auth_key") {
//            let auth_key = cookie.value().to_string();
//            if util::is_valid_auth_key(&auth_key, &app_state.env) {
//                return Ok(PageLayout);
//            }
//        }
//
//        Err((StatusCode::UNAUTHORIZED, "Unauthorized"))
//    }
//}

// endregion: --- PageLayout Extractor
