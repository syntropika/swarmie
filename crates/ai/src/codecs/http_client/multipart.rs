use bytes::Bytes;
use mime::Mime;
use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct Part {
    name: String,
    content: PartContent,
    filename: Option<String>,
    content_type: Option<Mime>,
}

#[derive(Clone, Debug)]
pub enum PartContent {
    Text(String),
    Binary(Bytes),
}

impl Part {
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: PartContent::Text(value.into()),
            filename: None,
            content_type: None,
        }
    }

    pub fn bytes(name: impl Into<String>, data: impl Into<Bytes>) -> Self {
        Self {
            name: name.into(),
            content: PartContent::Binary(data.into()),
            filename: None,
            content_type: None,
        }
    }

    pub fn filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    pub fn content_type(mut self, content_type: Mime) -> Self {
        self.content_type = Some(content_type);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn get_filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    pub fn get_content_type(&self) -> Option<&Mime> {
        self.content_type.as_ref()
    }

    pub fn content(&self) -> &PartContent {
        &self.content
    }

    pub fn into_pieces(self) -> (String, PartContent, Option<String>, Option<Mime>) {
        (self.name, self.content, self.filename, self.content_type)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MultipartForm {
    parts: Vec<Part>,
    boundary: Option<String>,
}

impl MultipartForm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn part(mut self, part: Part) -> Self {
        self.parts.push(part);
        self
    }

    pub fn text(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.part(Part::text(name, value))
    }

    pub fn file(
        self,
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: Mime,
        data: impl Into<Bytes>,
    ) -> Self {
        self.part(
            Part::bytes(name, data)
                .filename(filename)
                .content_type(content_type),
        )
    }

    pub fn boundary(mut self, boundary: impl Into<String>) -> Self {
        self.boundary = Some(boundary.into());
        self
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn into_parts(self) -> Vec<Part> {
        self.parts
    }

    fn generate_boundary() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("----boundary{timestamp}")
    }

    fn get_boundary(&self) -> Cow<'_, str> {
        match &self.boundary {
            Some(b) => Cow::Borrowed(b),
            None => Cow::Owned(Self::generate_boundary()),
        }
    }

    pub fn encode(&self) -> (String, Bytes) {
        let boundary = self.get_boundary();
        let mut body = Vec::new();

        for part in &self.parts {
            body.extend_from_slice(b"--");
            body.extend_from_slice(boundary.as_bytes());
            body.extend_from_slice(b"\r\n");

            body.extend_from_slice(b"Content-Disposition: form-data; name=\"");
            body.extend_from_slice(part.name.as_bytes());
            body.extend_from_slice(b"\"");

            if let Some(filename) = &part.filename {
                body.extend_from_slice(b"; filename=\"");
                body.extend_from_slice(filename.as_bytes());
                body.extend_from_slice(b"\"");
            }
            body.extend_from_slice(b"\r\n");

            if let Some(content_type) = &part.content_type {
                body.extend_from_slice(b"Content-Type: ");
                body.extend_from_slice(content_type.as_ref().as_bytes());
                body.extend_from_slice(b"\r\n");
            }

            body.extend_from_slice(b"\r\n");

            match &part.content {
                PartContent::Text(text) => body.extend_from_slice(text.as_bytes()),
                PartContent::Binary(bytes) => body.extend_from_slice(bytes),
            }

            body.extend_from_slice(b"\r\n");
        }

        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(b"--\r\n");

        (boundary.into_owned(), Bytes::from(body))
    }
}
