use std::{collections::BTreeSet, time::Duration};
use swarmie_device_linking::*;
use swarmie_permissions::transport::BootstrapStream;
use tokio::io::duplex;
use uuid::Uuid;

fn registration(keys: &EndpointKeys, account: Uuid) -> DeviceRegistration {
    DeviceRegistration {
        account,
        device: keys.id(),
        public_key: keys.public(),
        family: Uuid::new_v4(),
        generation: 1,
        name: "Registered client".into(),
        nodes: BTreeSet::new(),
    }
}

#[tokio::test]
async fn invalid_proof_preserves_the_offer_and_a_verified_claim_is_bound_to_the_actual_peer() {
    let target = EndpointKeys::generate();
    let client = EndpointKeys::generate();
    let account = Uuid::new_v4();
    let mut owner = OfferOwner::new(
        &target,
        account,
        "node-a".into(),
        "https://cloud.example".into(),
        Duration::from_secs(60),
    )
    .unwrap();
    let offer = owner.offer().unwrap();
    let code = offer.code().unwrap();
    assert!(LinkOffer::decode(&code, "https://other.example").is_err());
    let parsed = LinkOffer::decode(&code, "https://cloud.example").unwrap();
    let (a, b) = duplex(8192);
    let client_key = client.transport();
    let target_key = target.transport();
    let (a, b) = tokio::join!(
        BootstrapStream::connect(a, &client_key, parsed.target(), parsed.rendezvous()),
        BootstrapStream::accept(b, &target_key, offer.rendezvous()),
    );
    let a = a.unwrap();
    let b = b.unwrap();
    let registered = registration(&client, account);
    let proof = parsed.claim_proof(&a.binding()).unwrap();
    assert!(matches!(
        owner.claim(&b.binding(), &[0; 32], &registered),
        Err(Error::Forbidden)
    ));
    assert!(owner.offer().is_ok());
    let claim = owner.claim(&b.binding(), &proof, &registered).unwrap();
    assert_eq!(claim.claimant().name, "Registered client");
    assert!(matches!(owner.offer(), Err(Error::Claimed)));
    assert!(matches!(
        owner.claim(&b.binding(), &proof, &registered),
        Err(Error::Claimed)
    ));
    let intent = LinkIntent {
        operation: Uuid::new_v4(),
        nonce: Uuid::new_v4(),
        account,
        controller: client.id(),
        controller_generation: 1,
        target: target.id(),
        target_generation: 1,
        node: "node-a".into(),
        offer: offer.rendezvous(),
        vault: None,
    };
    claim.check(&intent).unwrap();
    assert!(
        claim
            .check(&LinkIntent {
                node: "another-node".into(),
                ..intent.clone()
            })
            .is_err()
    );
    drop(b);
    assert!(claim.check(&intent).is_err());
}

#[tokio::test]
async fn claim_proof_cannot_be_replayed_on_another_completed_channel() {
    let target = EndpointKeys::generate();
    let client = EndpointKeys::generate();
    let account = Uuid::new_v4();
    let mut owner = OfferOwner::new(
        &target,
        account,
        "node-a".into(),
        "https://cloud.example".into(),
        Duration::from_secs(60),
    )
    .unwrap();
    let offer = owner.offer().unwrap();
    let client_key = client.transport();
    let target_key = target.transport();
    let (a, b) = duplex(8192);
    let (c, d) = duplex(8192);
    let (a, b, c, d) = tokio::join!(
        BootstrapStream::connect(a, &client_key, offer.target(), offer.rendezvous()),
        BootstrapStream::accept(b, &target_key, offer.rendezvous()),
        BootstrapStream::connect(c, &client_key, offer.target(), offer.rendezvous()),
        BootstrapStream::accept(d, &target_key, offer.rendezvous()),
    );
    let a = a.unwrap();
    let _b = b.unwrap();
    let c = c.unwrap();
    let d = d.unwrap();
    let registered = registration(&client, account);
    let original = offer.claim_proof(&a.binding()).unwrap();
    assert!(matches!(
        owner.claim(&d.binding(), &original, &registered),
        Err(Error::Forbidden)
    ));
    assert!(owner.offer().is_ok());
    owner
        .claim(
            &d.binding(),
            &offer.claim_proof(&c.binding()).unwrap(),
            &registered,
        )
        .unwrap();
}

#[test]
fn registration_and_link_signatures_bind_the_exact_account_node_and_generations() {
    let client = EndpointKeys::generate();
    let target = EndpointKeys::generate();
    let account = Uuid::new_v4();
    let challenge = RegistrationChallenge {
        id: Uuid::new_v4(),
        account,
        family: Uuid::new_v4(),
        issuer: "https://cloud.example".into(),
        nonce: [1; 32],
        expires_at: 1000,
    };
    let proof = client
        .registration(challenge, "Client".into(), None, None)
        .unwrap();
    proof.verify().unwrap();
    assert!(
        RegistrationProof {
            name: "Substituted".into(),
            ..proof
        }
        .verify()
        .is_err()
    );
    let controller = registration(&client, account);
    let mut recipient = registration(&target, account);
    recipient.nodes.insert("node-a".into());
    let intent = LinkIntent {
        operation: Uuid::new_v4(),
        nonce: Uuid::new_v4(),
        account,
        controller: client.id(),
        controller_generation: 1,
        target: target.id(),
        target_generation: 1,
        node: "node-a".into(),
        offer: [1; 16],
        vault: None,
    };
    let signed = SignedLink {
        controller_signature: client.sign_link(&intent).unwrap(),
        target_signature: target.sign_link(&intent).unwrap(),
        intent,
    };
    signed.verify(&controller, &recipient).unwrap();
    let altered = SignedLink {
        intent: LinkIntent {
            node: "other-node".into(),
            ..signed.intent.clone()
        },
        ..signed.clone()
    };
    assert!(altered.verify(&controller, &recipient).is_err());
    recipient.generation += 1;
    assert!(signed.verify(&controller, &recipient).is_err());
    let encoded: String = client.id().into();
    assert_eq!(DeviceId::try_from(encoded.clone()).unwrap(), client.id());
    assert!(DeviceId::try_from(encoded.to_uppercase()).is_err());
}

#[tokio::test]
async fn valid_capability_is_consumed_before_a_foreign_account_is_refused() {
    let target = EndpointKeys::generate();
    let client = EndpointKeys::generate();
    let account = Uuid::new_v4();
    let mut owner = OfferOwner::new(
        &target,
        account,
        "node-a".into(),
        "https://cloud.example".into(),
        Duration::from_secs(60),
    )
    .unwrap();
    let offer = owner.offer().unwrap();
    let (a, b) = duplex(8192);
    let client_key = client.transport();
    let target_key = target.transport();
    let (a, b) = tokio::join!(
        BootstrapStream::connect(a, &client_key, offer.target(), offer.rendezvous()),
        BootstrapStream::accept(b, &target_key, offer.rendezvous())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    let proof = offer.claim_proof(&a.binding()).unwrap();
    let claimed = owner.claim_key(&b.binding(), &proof).unwrap();
    assert_eq!(claimed.device(), client.id());
    assert!(matches!(owner.offer(), Err(Error::Claimed)));
    let foreign = registration(&client, Uuid::new_v4());
    assert!(matches!(claimed.authorize(&foreign), Err(Error::Forbidden)));
    assert!(matches!(
        owner.claim_key(&b.binding(), &proof),
        Err(Error::Claimed)
    ));
}
