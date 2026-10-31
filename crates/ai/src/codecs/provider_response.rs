use http::StatusCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResponseError {
    pub status: Option<StatusCode>,
    pub body: String,
    pub provider_request_id: Option<String>,
    pub headers: Option<http::HeaderMap>,
    pub code: Option<String>,
    pub transient: Option<bool>,
    pub refusal: bool,
    pub route: Option<String>,
}

impl ProviderResponseError {
    pub fn new(status: StatusCode, body: impl Into<String>) -> Self {
        let status = Some(status);
        Self {
            status,
            ..Self::without_status(body)
        }
    }

    pub fn without_status(body: impl Into<String>) -> Self {
        Self {
            status: None,
            body: body.into(),
            provider_request_id: None,
            headers: None,
            code: None,
            transient: None,
            refusal: false,
            route: None,
        }
    }

    pub fn from_body(body: impl Into<String>) -> Self {
        let mut reply = Self::without_status(body);
        let status = reply.located(|envelope| envelope.http_status().filter(|_| envelope.nested));
        reply.status = status.and_then(|status| StatusCode::from_u16(status).ok());
        reply
    }

    pub fn with_refusal(mut self, refusal: bool) -> Self {
        self.refusal = refusal;
        self
    }

    pub fn with_status(mut self, status: Option<StatusCode>) -> Self {
        if self.status.is_none() {
            self.status = status;
        }
        self
    }

    pub fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code.filter(|code| !code.is_empty());
        self
    }

    pub fn with_transient(mut self, transient: Option<bool>) -> Self {
        self.transient = transient;
        self
    }

    pub fn is_retryable(&self) -> bool {
        if self.refusal {
            return false;
        }
        if let Some(status) = self.status.filter(|status| !status.is_success()) {
            return crate::codecs::error::retryable_status(Some(status.as_u16()));
        }
        let known = match &self.code {
            Some(code) => Envelope::default().named(code).verdict(),
            None => self.located(|envelope| envelope.verdict()),
        };
        known.or(self.transient).unwrap_or(false)
    }

    pub fn machine_code(&self) -> Option<String> {
        self.code
            .clone()
            .or_else(|| self.located(|envelope| envelope.code()))
    }

    pub fn envelope_json(&self) -> Option<serde_json::Value> {
        let doc: serde_json::Value = serde_json::from_str(&self.body).ok()?;
        let envelope = envelope(&doc)?;
        let error = envelope.error?.clone();
        Some(match envelope.nested {
            true => serde_json::json!({ "error": error }),
            false => error,
        })
    }

    fn located<T>(&self, f: impl FnOnce(Envelope<'_>) -> Option<T>) -> Option<T> {
        let doc: serde_json::Value = serde_json::from_str(&self.body).ok()?;
        delivered(&doc).and_then(f)
    }

    pub fn with_provider_request_id(mut self, request_id: Option<String>) -> Self {
        self.provider_request_id = request_id.filter(|id| !id.is_empty());
        self
    }

    pub fn with_headers(mut self, headers: Option<http::HeaderMap>) -> Self {
        self.headers = headers;
        self
    }
}

impl std::fmt::Display for ProviderResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(status) => write!(f, "status {status}: {}", self.body)?,
            None => write!(f, "{}", self.body)?,
        }
        // The id support asks for belongs in the message a caller logs.
        if let Some(request_id) = &self.provider_request_id {
            write!(f, " (request id: {request_id})")?;
        }
        if let Some(route) = &self.route {
            write!(f, " [{route}]")?;
        }
        Ok(())
    }
}

impl std::error::Error for ProviderResponseError {}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderResponseErrorWire {
    status: Option<u16>,
    body: String,
    provider_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transient: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    refusal: bool,
}

impl serde::Serialize for ProviderResponseError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ProviderResponseErrorWire {
            status: self.status.map(|status| status.as_u16()),
            body: self.body.clone(),
            provider_request_id: self.provider_request_id.clone(),
            code: self.code.clone(),
            transient: self.transient,
            refusal: self.refusal,
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ProviderResponseError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let wire = ProviderResponseErrorWire::deserialize(deserializer)?;
        let status = wire
            .status
            .map(StatusCode::from_u16)
            .transpose()
            .map_err(D::Error::custom)?;
        Ok(Self {
            status,
            body: wire.body,
            provider_request_id: wire.provider_request_id,
            headers: None,
            code: wire.code,
            transient: wire.transient,
            refusal: wire.refusal,
            route: None,
        })
    }
}

#[derive(Default)]
struct Envelope<'a> {
    error: Option<&'a serde_json::Value>,
    name: Option<&'a str>,
    number: Option<i64>,
    nested: bool,
}

pub(crate) fn located_error(doc: &serde_json::Value) -> Option<(&serde_json::Value, bool)> {
    envelope(doc).and_then(|envelope| Some((envelope.error?, envelope.nested)))
}

const TRANSIENT: &[&str] = &[
    "overloaded",
    "overloaded_error",
    "api_error",
    "rate_limit_error",
    "rate_limit_exceeded",
    "too_many_requests",
    "server_error",
    "server_is_overloaded",
    "slow_down",
    "unavailable",
    "resource_exhausted",
    "deadline_exceeded",
    "aborted",
    "ThrottlingException",
    "ServiceUnavailableException",
    "InternalServerException",
    "ModelNotReadyException",
    "ModelTimeoutException",
    "ModelStreamErrorException",
];

const FINAL: &[&str] = &[
    "insufficient_quota",
    "quota_exceeded",
    "billing_hard_limit_reached",
    "context_length_exceeded",
    "invalid_prompt",
];

fn envelope(doc: &serde_json::Value) -> Option<Envelope<'_>> {
    use serde_json::Value;
    const FIELDS: &[&str] = &["code", "status", "type"];
    let object = |value: &&Value| value.as_object().is_some_and(|fields| !fields.is_empty());
    if let Some(error) = doc.pointer("/response/error").filter(object) {
        return Some(Envelope::of(error, FIELDS, true));
    }
    match doc.get("error") {
        Some(error) if object(&error) => Some(Envelope::of(error, FIELDS, true)),
        Some(error @ Value::String(message)) if !message.is_empty() => {
            Some(Envelope::of(error, &[], true))
        }
        _ => (doc.get("type").and_then(Value::as_str) == Some("error"))
            .then(|| Envelope::of(doc, &["code"], false)),
    }
}

fn delivered(doc: &serde_json::Value) -> Option<Envelope<'_>> {
    if let Some(items) = doc.as_array() {
        return items.iter().find_map(delivered);
    }
    let message = doc.get("message").and_then(serde_json::Value::as_str);
    let bare = message.is_some_and(|message| !message.is_empty())
        && doc.get("error").is_none()
        && doc.get("response").is_none();
    envelope(doc).or_else(|| bare.then(|| Envelope::of(doc, &["code", "status", "type"], false)))
}

impl<'a> Envelope<'a> {
    fn named(mut self, name: &'a str) -> Self {
        self.name = Some(name);
        self
    }

    fn of(error: &'a serde_json::Value, fields: &[&str], nested: bool) -> Self {
        let values = || fields.iter().filter_map(|field| error.get(field));
        Self {
            error: Some(error),
            name: values().find_map(|value| value.as_str().filter(|name| !name.is_empty())),
            number: values().find_map(serde_json::Value::as_i64),
            nested,
        }
    }

    fn code(&self) -> Option<String> {
        let number = || self.number.map(|number| number.to_string());
        self.name.map(str::to_owned).or_else(number)
    }

    fn verdict(&self) -> Option<bool> {
        let named = |codes: &[&str]| {
            let name = self.name.unwrap_or_default();
            codes.iter().any(|code| code.eq_ignore_ascii_case(name))
        };
        let status = || {
            self.http_status()
                .map(|s| crate::codecs::error::retryable_status(Some(s)))
        };
        (named(TRANSIENT).then_some(true))
            .or_else(|| named(FINAL).then_some(false))
            .or_else(status)
    }

    fn http_status(&self) -> Option<u16> {
        let number = u16::try_from(self.number?).ok();
        number.filter(|number| (400..=599).contains(number))
    }
}

pub(crate) fn reported(id: Option<String>) -> Option<String> {
    id.filter(|id| !id.is_empty())
}

pub(crate) fn json(body: Option<&str>) -> Result<Option<serde_json::Value>, serde_json::Error> {
    body.filter(|body| !body.is_empty())
        .map(serde_json::from_str)
        .transpose()
}
