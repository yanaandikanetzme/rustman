#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastPlacement {
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub placement: ToastPlacement,
    pub id: String,
    pub title: String,
    pub detail: Option<String>,
}
