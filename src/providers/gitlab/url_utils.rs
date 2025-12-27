pub fn pipeline_id_to_url(base_url: &str, project_path: &str, gid: &str) -> String {
    let id = extract_numeric_id(gid);
    format!("{base_url}/{project_path}/-/pipelines/{id}")
}

pub fn job_id_to_url(base_url: &str, project_path: &str, gid: &str) -> String {
    let id = extract_numeric_id(gid);
    format!("{base_url}/{project_path}/-/jobs/{id}")
}

fn extract_numeric_id(gid: &str) -> &str {
    // GitLab GIDs format: gid://gitlab/Ci::Pipeline/123 or gid://gitlab/Ci::Job/456
    // Extract the numeric ID after the last slash
    gid.rsplit('/').next().unwrap_or(gid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_numeric_id_pipeline() {
        assert_eq!(extract_numeric_id("gid://gitlab/Ci::Pipeline/123"), "123");
    }

    #[test]
    fn test_extract_numeric_id_job() {
        assert_eq!(extract_numeric_id("gid://gitlab/Ci::Job/456"), "456");
    }

    #[test]
    fn test_pipeline_id_to_url() {
        let url = pipeline_id_to_url(
            "https://gitlab.com",
            "group/project",
            "gid://gitlab/Ci::Pipeline/123456",
        );
        assert_eq!(url, "https://gitlab.com/group/project/-/pipelines/123456");
    }

    #[test]
    fn test_job_id_to_url() {
        let url = job_id_to_url(
            "https://gitlab.com",
            "group/project",
            "gid://gitlab/Ci::Job/789012",
        );
        assert_eq!(url, "https://gitlab.com/group/project/-/jobs/789012");
    }
}
