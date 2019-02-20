use crate::auth::{encode_token, Claims};
use crate::conduit;
use crate::db::Repo;
use crate::models::User;
use crate::models::*;
use crate::schema::users;
use diesel::prelude::*;
use http::status::StatusCode;
use jsonwebtoken::{encode, Algorithm, Header};
use tide::{self, body::Json, AppData};

#[derive(Deserialize, Debug)]
pub struct Registration {
    user: NewUser,
}

#[derive(Deserialize, Debug)]
pub struct UpdateUserRequest {
    user: UpdateUser,
}

#[derive(Deserialize, Debug, AsChangeset)]
#[table_name = "users"]
pub struct UpdateUser {
    email: Option<String>,
    username: Option<String>,
    password: Option<String>,
    image: Option<String>,
    bio: Option<String>,
}

#[derive(Serialize)]
pub struct UserResponse {
    user: User,
}

#[derive(Deserialize)]
pub struct AuthRequest {
    user: AuthUser,
}

#[derive(Deserialize)]
pub struct AuthUser {
    email: String,
    password: String,
}

pub async fn register(
    repo: AppData<Repo>,
    registration: Json<Registration>,
) -> Result<Json<UserResponse>, StatusCode> {
    use crate::schema::users;

    let result = await! { conduit::create_user(repo.clone(), registration.0.user) };

    result
        .map(|user| Json(UserResponse { user }))
        .map_err(|e| diesel_error(&e))
}

pub async fn login(
    repo: AppData<Repo>,
    auth: Json<AuthRequest>,
) -> Result<Json<UserResponse>, StatusCode> {
    use crate::schema::users::dsl::*;

    let user = auth.0.user;
    let result = await! { repo.run(|conn| {
    users
        .filter(email.eq(user.email))
        .filter(password.eq(user.password))
        .first::<User>(&conn)
    }) };

    match result {
        Ok(user) => {
            let user = User {
                token: Some(encode_token(user.id)),
                ..user
            };
            Ok(Json(UserResponse { user }))
        }
        Err(diesel::result::Error::NotFound) => Err(StatusCode::UNAUTHORIZED),
        Err(e) => Err(diesel_error(&e)),
    }
}

pub async fn get_user(repo: AppData<Repo>, auth: Claims) -> Result<Json<UserResponse>, StatusCode> {
    use crate::schema::users::dsl::*;
    let user_id = auth.user_id();
    info!("Get user {}", user_id);

    let results = await! { repo.run(move |conn| users.find(user_id).first(&conn)) };

    results
        .map(|user| Json(UserResponse { user }))
        .map_err(|e| diesel_error(&e))
}

pub async fn update_user(
    repo: AppData<Repo>,
    update_params: Json<UpdateUserRequest>,
    auth: Claims,
) -> Result<Json<UserResponse>, StatusCode> {
    use crate::schema::users::dsl::*;
    let user_id = auth.user_id();
    info!("Update user {} {:?}", user_id, update_params.0);

    let results = await! { repo.run(move |conn| {
        diesel::update(users.find(user_id))
            .set(&update_params.0.user)
            .get_result(&conn)
    })};

    results
        .map(|user| Json(UserResponse { user }))
        .map_err(|e| diesel_error(&e))
}

pub async fn list_articles(repo: AppData<Repo>) -> Result<Json<Vec<Article>>, StatusCode> {
    use crate::schema::articles::dsl::*;

    let results = await! { repo.run(|conn| articles.limit(10).load::<Article>(&conn)) };

    results
        .map(|article_list| Json(article_list))
        .map_err(|e| diesel_error(&e))
}

fn diesel_error(e: &diesel::result::Error) -> StatusCode {
    error!("{}", e);
    StatusCode::INTERNAL_SERVER_ERROR
}

#[cfg(test)]
mod tests {
    // These tests are more like "integration" tests that exercise a workflow via the tide handlers.
    use super::*;
    use crate::auth;
    use crate::schema::users;
    use crate::schema::users::dsl::*;
    use crate::test_helpers::{generate, init_env};
    use diesel::prelude::*;
    use fake::fake;
    use tokio_async_await_test::async_test;

    #[async_test]
    async fn register_and_login() {
        init_env();
        let repo = Repo::new();
        let user = generate::new_user();

        let stored_user = await! { register_user(repo.clone(), user.clone()) };
        let auth = await! { login_user(repo.clone(), user.clone()) };

        let user_details = await! { get_user(AppData(repo.clone()), auth)}
            .expect("Get user failed")
            .0
            .user;
        assert_eq!(user_details.username, user.username);
        assert_eq!(user_details.email, user.email);
    }

    async fn register_user(repo: Repo, user: NewUser) -> User {
        let reg_request = Json(Registration { user: user });
        let reg_response = await! { register(AppData(repo), reg_request) };
        reg_response.expect("Registration failed").0.user
    }

    async fn login_user(repo: Repo, user: NewUser) -> Claims {
        let login_request = Json(AuthRequest {
            user: AuthUser {
                email: user.email,
                password: user.password,
            },
        });

        let login_response = await! { login(AppData(repo), login_request)};
        let stored_user = login_response.expect("User login failed").0.user;
        assert!(stored_user.token.is_some());

        auth::claims_for(stored_user.id, 60)
    }
}