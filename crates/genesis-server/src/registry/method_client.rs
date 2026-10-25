//! Typed source exchange; neither discovery nor transport grants execution rights.
use super::methods::{
    reference_ok, text_ok, MethodCandidate, MethodLookup, MethodOffer, MethodPublication,
    MethodSearch, MethodSearchResults,
};
use super::RegistryClient;
use crate::error::{ApiError, ApiResult};
use axum::http::StatusCode;
use genesis_core::ids::OrgId;
use std::collections::HashSet;

fn invalid_response() -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "registry_invalid_response",
        "registry method response is invalid or inconsistent",
    )
}

fn validate_offer(offer: &MethodOffer) -> ApiResult<()> {
    let package = offer
        .publication
        .validate()
        .map_err(|_| invalid_response())?;
    offer
        .candidate(&package)
        .validate()
        .map_err(|_| invalid_response())
}

fn validate_candidate(candidate: &MethodCandidate) -> ApiResult<()> {
    candidate.validate().map_err(|_| invalid_response())?;
    let mut dependencies = HashSet::new();
    if candidate
        .dependencies
        .iter()
        .any(|r| !dependencies.insert((&r.domain, &r.schema, &r.id, &r.version)))
    {
        return Err(invalid_response());
    }
    Ok(())
}

impl RegistryClient {
    /// Publish public source once. The configured token determines the remote
    /// publisher. This expectation is sent as a pre-write check and also checked
    /// against the response. Older servers may ignore it; transport/response
    /// errors can still follow a committed write, so reconcile before retrying.
    pub async fn publish_method(
        &self,
        expected_publisher: &OrgId,
        publication: &MethodPublication,
    ) -> ApiResult<MethodOffer> {
        let expected = MethodOffer {
            publisher: expected_publisher.clone(),
            publication: publication.clone(),
        };
        validate_offer(&expected).map_err(|_| Self::invalid_request())?;
        let url = self.endpoint(&["registry", "v1", "methods"])?;
        let response = self
            .http
            .post(url)
            .query(&[("expected_publisher", expected_publisher.as_str())])
            .bearer_auth(&self.token)
            .json(publication)
            .send()
            .await
            .map_err(Self::unreachable)?;
        let offer: MethodOffer = Self::decode(response).await?;
        if offer != expected {
            return Err(invalid_response());
        }
        Ok(offer)
    }

    /// Re-fetch the exact publisher-qualified source selected by the receiver.
    /// This checks source identity, not a prior search hit's metadata or acceptance.
    pub async fn method_offer(&self, lookup: &MethodLookup) -> ApiResult<MethodOffer> {
        if !text_ok(lookup.publisher.as_str(), 256)
            || !reference_ok(&lookup.reference)
            || lookup.reference.domain != "genesis"
            || lookup.reference.schema != "development-method-source/1"
        {
            return Err(Self::invalid_request());
        }
        let url = self.endpoint(&["registry", "v1", "methods", "fetch"])?;
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(lookup)
            .send()
            .await
            .map_err(Self::unreachable)?;
        let offer: MethodOffer = Self::decode(response).await?;
        if offer.publisher != lookup.publisher || offer.publication.reference != lookup.reference {
            return Err(invalid_response());
        }
        validate_offer(&offer)?;
        Ok(offer)
    }

    /// Fetch a selected search hit and require its complete metadata and declared
    /// environment to match the canonical source. This still grants no authority.
    pub async fn fetch_method_candidate(
        &self,
        candidate: &MethodCandidate,
    ) -> ApiResult<MethodOffer> {
        validate_candidate(candidate).map_err(|_| Self::invalid_request())?;
        let offer = self
            .method_offer(&MethodLookup {
                publisher: candidate.publisher.clone(),
                reference: candidate.reference.clone(),
            })
            .await?;
        let package = offer
            .publication
            .validate()
            .map_err(|_| invalid_response())?;
        if offer.candidate(&package) != *candidate {
            return Err(invalid_response());
        }
        Ok(offer)
    }

    /// Return bounded, source-free candidates; a failed or inconsistent response
    /// is an error, never evidence of absence. Candidates still require a fetch.
    pub async fn search_methods(&self, request: &MethodSearch) -> ApiResult<MethodSearchResults> {
        request.validate().map_err(|_| Self::invalid_request())?;
        let url = self.endpoint(&["registry", "v1", "methods", "search"])?;
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(request)
            .send()
            .await
            .map_err(Self::unreachable)?;
        let results: MethodSearchResults = Self::decode(response).await?;
        if results.candidates.len() > request.limit
            || (results.truncated && results.candidates.len() != request.limit)
            || results.candidates.windows(2).any(|pair| {
                let a = &pair[0];
                let b = &pair[1];
                (a.publisher.as_str(), &a.reference.id, &a.reference.version)
                    >= (b.publisher.as_str(), &b.reference.id, &b.reference.version)
            })
        {
            return Err(invalid_response());
        }
        let query = request.query.to_lowercase();
        let mut identities = HashSet::new();
        for candidate in &results.candidates {
            validate_candidate(candidate)?;
            let r = &candidate.reference;
            if !identities.insert((
                &candidate.publisher,
                &r.domain,
                &r.schema,
                &r.id,
                &r.version,
            )) || request
                .runtime
                .as_ref()
                .is_some_and(|r| r != &candidate.runtime)
                || request
                    .protocol
                    .as_ref()
                    .is_some_and(|r| r != &candidate.protocol)
                || request.exclude_publisher.as_ref() == Some(&candidate.publisher)
                || !format!("{} {}", candidate.intent, candidate.applicability.join(" "))
                    .to_lowercase()
                    .contains(&query)
            {
                return Err(invalid_response());
            }
        }
        Ok(results)
    }
}
