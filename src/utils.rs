use uuid::Uuid;

pub fn new_context_id() -> String {
    Uuid::new_v4().to_string()
}
