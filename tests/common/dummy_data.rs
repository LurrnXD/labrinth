use actix_web::test::{self, TestRequest};
use labrinth::{models::projects::Project, models::projects::Version};
use serde_json::json;
use sqlx::Executor;

use crate::common::{
    actix::AppendsMultipart,
    database::{MOD_USER_PAT, USER_USER_PAT},
};

use super::{
    actix::{MultipartSegment, MultipartSegmentData},
    environment::TestEnvironment,
};

pub const DUMMY_CATEGORIES: &'static [&str] = &[
    "combat",
    "decoration",
    "economy",
    "food",
    "magic",
    "mobs",
    "optimization",
];

pub struct DummyData {
    pub alpha_team_id: String,
    pub beta_team_id: String,

    pub alpha_project_id: String,
    pub beta_project_id: String,

    pub alpha_project_slug: String,
    pub beta_project_slug: String,

    pub alpha_version_id: String,
    pub beta_version_id: String,

    pub alpha_thread_id: String,
    pub beta_thread_id: String,

    pub alpha_file_hash: String,
    pub beta_file_hash: String,
}

pub async fn add_dummy_data(test_env: &TestEnvironment) -> DummyData {
    // Adds basic dummy data to the database directly with sql (user, pats)
    let pool = &test_env.db.pool.clone();
    pool.execute(include_str!("../files/dummy_data.sql"))
        .await
        .unwrap();

    let (alpha_project, alpha_version) = add_project_alpha(test_env).await;
    let (beta_project, beta_version) = add_project_beta(test_env).await;

    DummyData {
        alpha_team_id: alpha_project.team.to_string(),
        beta_team_id: beta_project.team.to_string(),

        alpha_project_id: alpha_project.id.to_string(),
        beta_project_id: beta_project.id.to_string(),

        alpha_project_slug: alpha_project.slug.unwrap(),
        beta_project_slug: beta_project.slug.unwrap(),

        alpha_version_id: alpha_version.id.to_string(),
        beta_version_id: beta_version.id.to_string(),

        alpha_thread_id: alpha_project.thread_id.to_string(),
        beta_thread_id: beta_project.thread_id.to_string(),

        alpha_file_hash: alpha_version.files[0].hashes["sha1"].clone(),
        beta_file_hash: beta_version.files[0].hashes["sha1"].clone(),
    }
}

pub async fn add_project_alpha(test_env: &TestEnvironment) -> (Project, Version) {
    // Adds dummy data to the database with sqlx (projects, versions, threads)
    // Generate test project data.
    let json_data = json!(
        {
            "title": "Test Project Alpha",
            "slug": "alpha",
            "description": "A dummy project for testing with.",
            "body": "This project is approved, and versions are listed.",
            "client_side": "required",
            "server_side": "optional",
            "initial_versions": [{
                "file_parts": ["dummy-project-alpha.jar"],
                "version_number": "1.2.3",
                "version_title": "start",
                "dependencies": [],
                "game_versions": ["1.20.1"] ,
                "release_channel": "release",
                "loaders": ["fabric"],
                "featured": true
            }],
            "categories": [],
            "license_id": "MIT"
        }
    );

    // Basic json
    let json_segment = MultipartSegment {
        name: "data".to_string(),
        filename: None,
        content_type: Some("application/json".to_string()),
        data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
    };

    // Basic file
    let file_segment = MultipartSegment {
        name: "dummy-project-alpha.jar".to_string(),
        filename: Some("dummy-project-alpha.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: MultipartSegmentData::Binary(
            include_bytes!("../../tests/files/dummy-project-alpha.jar").to_vec(),
        ),
    };

    // Add a project.
    let req = TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", USER_USER_PAT))
        .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 200);

    // Approve as a moderator.
    let req = TestRequest::patch()
        .uri("/v2/project/alpha")
        .append_header(("Authorization", MOD_USER_PAT))
        .set_json(json!(
            {
                "status": "approved"
            }
        ))
        .to_request();
    let resp = test_env.call(req).await;
    assert_eq!(resp.status(), 204);

    // Get project
    let req = TestRequest::get()
        .uri("/v2/project/alpha")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let project: Project = test::read_body_json(resp).await;

    // Get project's versions
    let req = TestRequest::get()
        .uri("/v2/project/alpha/version")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let versions: Vec<Version> = test::read_body_json(resp).await;
    let version = versions.into_iter().next().unwrap();

    (project, version)
}

pub async fn add_project_beta(test_env: &TestEnvironment) -> (Project, Version) {
    // Adds dummy data to the database with sqlx (projects, versions, threads)
    // Generate test project data.
    let json_data = json!(
        {
            "title": "Test Project Beta",
            "slug": "beta",
            "description": "A dummy project for testing with.",
            "body": "This project is not-yet-approved, and versions are draft.",
            "client_side": "required",
            "server_side": "optional",
            "initial_versions": [{
                "file_parts": ["dummy-project-beta.jar"],
                "version_number": "1.2.3",
                "version_title": "start",
                "status": "unlisted",
                "requested_status": "unlisted",
                "dependencies": [],
                "game_versions": ["1.20.1"] ,
                "release_channel": "release",
                "loaders": ["fabric"],
                "featured": true
            }],
            "status": "private",
            "requested_status": "private",
            "categories": [],
            "license_id": "MIT"
        }
    );

    // Basic json
    let json_segment = MultipartSegment {
        name: "data".to_string(),
        filename: None,
        content_type: Some("application/json".to_string()),
        data: MultipartSegmentData::Text(serde_json::to_string(&json_data).unwrap()),
    };

    // Basic file
    let file_segment = MultipartSegment {
        name: "dummy-project-beta.jar".to_string(),
        filename: Some("dummy-project-beta.jar".to_string()),
        content_type: Some("application/java-archive".to_string()),
        data: MultipartSegmentData::Binary(
            include_bytes!("../../tests/files/dummy-project-beta.jar").to_vec(),
        ),
    };

    // Add a project.
    let req = TestRequest::post()
        .uri("/v2/project")
        .append_header(("Authorization", USER_USER_PAT))
        .set_multipart(vec![json_segment.clone(), file_segment.clone()])
        .to_request();
    let resp = test_env.call(req).await;

    assert_eq!(resp.status(), 200);

    // Get project
    let req = TestRequest::get()
        .uri("/v2/project/beta")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let project: Project = test::read_body_json(resp).await;

    // Get project's versions
    let req = TestRequest::get()
        .uri("/v2/project/beta/version")
        .append_header(("Authorization", USER_USER_PAT))
        .to_request();
    let resp = test_env.call(req).await;
    let versions: Vec<Version> = test::read_body_json(resp).await;
    let version = versions.into_iter().next().unwrap();

    (project, version)
}