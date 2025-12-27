#[derive(Debug)]
pub struct GitLabPipeline {
    pub id: String,
    pub ref_: String,
    pub source: String,
    pub status: String,
    pub duration: usize,
    pub stages: Vec<String>,
    pub jobs: Vec<GitLabJob>,
}

#[derive(Debug)]
pub struct GitLabJob {
    pub id: String,
    pub name: String,
    pub stage: String,
    pub duration: f64,
    pub status: String,
    pub retried: bool,
    pub needs: Option<Vec<String>>,
}
