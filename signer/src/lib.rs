#![cfg_attr(not(test), deny(clippy::expect_used,))]

use lazy_static::lazy_static;
use std::convert::TryFrom;
use std::str::FromStr;

use bip39::{Language, MnemonicType, Seed};
use bls_signatures::Serialize;
use fvm_shared::crypto::signature::{Signature, SignatureType};
use fvm_shared::message::Message;
use libsecp256k1::util::{COMPRESSED_PUBLIC_KEY_SIZE, SECRET_KEY_SIZE, SIGNATURE_SIZE};
use num_traits::FromPrimitive;
use rayon::prelude::*;
use zx_bip44::BIP44Path;

use cid::Cid;
use fil_actor_init::{ExecParams, Method as MethodInit};
use fil_actor_multisig as multisig;
use fil_actor_paych as paych;
use fvm_ipld_encoding::{from_slice, to_vec, Cbor, RawBytes};
use fvm_shared::address::{Address, Network, Protocol};

use bls_signatures::PublicKey as BLSPublicKey;
use libsecp256k1::PublicKey as SECP256K1PublicKey;

use extras::signed_message::ref_fvm::SignedMessage;
use regex::bytes::Regex;

use fvm_shared::econ::TokenAmount;
use fvm_shared::MethodNum;

use crate::api::{MessageParams, MessageTx, MessageTxAPI, MessageTxNetwork};
use crate::error::SignerError;
use crate::extended_key::ExtendedSecretKey;
use crate::multisig_deprecated::ConstructorParamsV1;

pub mod api;
pub mod error;
pub mod extended_key;
pub mod multisig_deprecated;
pub mod utils;

/// Mnemonic string
pub struct Mnemonic(pub String);

pub const SIGNATURE_RECOVERY_SIZE: usize = SIGNATURE_SIZE + 1;

/// Private key buffer
pub struct PrivateKey(pub [u8; SECRET_KEY_SIZE]);

pub enum PublicKey {
    SECP256K1PublicKey(SECP256K1PublicKey),
    BLSPublicKey(BLSPublicKey),
}

impl PublicKey {
    pub fn to_vec(&self) -> Vec<u8> {
        match self {
            // Uncompressed public key 65 bytes
            PublicKey::SECP256K1PublicKey(pk) => pk.serialize().to_vec(),
            PublicKey::BLSPublicKey(pk) => pk.as_bytes(),
        }
    }
}

/// Compressed public key buffer
pub struct PublicKeyCompressed(pub [u8; COMPRESSED_PUBLIC_KEY_SIZE]);

/// Extended key structure
pub struct ExtendedKey {
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
    pub address: String,
}

#[cfg(feature = "with-ffi-support")]
ffi_support::implement_into_ffi_by_pointer!(ExtendedKey);

impl TryFrom<String> for PrivateKey {
    type Error = SignerError;

    fn try_from(s: String) -> Result<PrivateKey, Self::Error> {
        let v = base64::decode(&s)?;

        PrivateKey::try_from(v)
    }
}

impl TryFrom<Vec<u8>> for PrivateKey {
    type Error = SignerError;

    fn try_from(v: Vec<u8>) -> Result<PrivateKey, Self::Error> {
        if v.len() != SECRET_KEY_SIZE {
            return Err(SignerError::GenericString("Invalid Key Length".to_string()));
        }
        let mut sk = PrivateKey([0; SECRET_KEY_SIZE]);
        sk.0.copy_from_slice(&v[..SECRET_KEY_SIZE]);
        Ok(sk)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProposalHashDataAPI {
    #[serde(with = "extras::json::option_address")]
    pub requester: Option<Address>,
    #[serde(with = "extras::json::address")]
    pub to: Address,
    #[serde(with = "extras::json::tokenamount")]
    pub value: TokenAmount,
    pub method: MethodNum,
    #[serde(with = "extras::json::rawbytes")]
    pub params: RawBytes,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SignedVoucherWrapper(
    #[serde(with = "extras::paych::SignedVoucherAPI")] paych::SignedVoucher,
);

/// Generates a random mnemonic (English - 24 words)
pub fn key_generate_mnemonic() -> Result<Mnemonic, SignerError> {
    let mnemonic = bip39::Mnemonic::new(MnemonicType::Words24, Language::English);
    Ok(Mnemonic(mnemonic.to_string()))
}

fn derive_extended_secret_key(seed: &[u8], path: &str) -> Result<ExtendedSecretKey, SignerError> {
    let master = ExtendedSecretKey::try_from(seed)?;
    let bip44_path = BIP44Path::from_string(path)?;
    let esk = master.derive_bip44(&bip44_path)?;

    Ok(esk)
}

fn derive_extended_secret_key_from_mnemonic(
    mnemonic: &str,
    path: &str,
    password: &str,
    language_code: &str,
) -> Result<ExtendedSecretKey, SignerError> {
    let lang = Language::from_language_code(language_code);

    match lang {
        Some(l) => {
            let mnemonic = bip39::Mnemonic::from_phrase(mnemonic, l)
                .map_err(|err| SignerError::GenericString(err.to_string()))?;

            let seed = Seed::new(&mnemonic, password);

            derive_extended_secret_key(seed.as_bytes(), path)
        }
        None => Err(SignerError::GenericString(
            "Unknown language code".to_string(),
        )),
    }
}

/// Returns a public key, private key and address given a mnemonic, derivation path and a password (support chinese mnemonic)
///
/// # Arguments
///
/// * `mnemonic` - A string containing a 24-words English mnemonic
/// * `path` - A string containing a derivation path
/// * `password` - Password to decrypt seed, if none use and empty string (e.g "")
/// * `language_code` - The language code for the mnemonic (e.g "en" if english words are used)
pub fn key_derive(
    mnemonic: &str,
    path: &str,
    password: &str,
    language_code: &str,
) -> Result<ExtendedKey, SignerError> {
    let esk = derive_extended_secret_key_from_mnemonic(mnemonic, path, password, language_code)?;

    let mut address = Address::new_secp256k1(esk.public_key().as_ref())?;

    let bip44_path = BIP44Path::from_string(path)?;

    address.set_network(Network::Mainnet);
    if bip44_path.is_testnet() {
        address.set_network(Network::Testnet);
    }

    Ok(ExtendedKey {
        private_key: PrivateKey(esk.secret_key()),
        public_key: PublicKey::SECP256K1PublicKey(SECP256K1PublicKey::parse(&esk.public_key())?),
        address: address.to_string(),
    })
}

/// Returns a public key, private key and address given a seed and derivation path
///
/// # Arguments
///
/// * `seed` - A seed as bytes array
/// * `path` - A string containing a derivation path
///
pub fn key_derive_from_seed(seed: &[u8], path: &str) -> Result<ExtendedKey, SignerError> {
    let esk = derive_extended_secret_key(seed, path)?;

    let mut address = Address::new_secp256k1(esk.public_key().as_ref())?;

    let bip44_path = BIP44Path::from_string(path)?;

    address.set_network(Network::Mainnet);
    if bip44_path.is_testnet() {
        address.set_network(Network::Testnet);
    }

    Ok(ExtendedKey {
        private_key: PrivateKey(esk.secret_key()),
        public_key: PublicKey::SECP256K1PublicKey(SECP256K1PublicKey::parse(&esk.public_key())?),
        address: address.to_string(),
    })
}

/// Get extended key from private key
///
/// # Arguments
///
/// * `private_key` - A `PrivateKey`
/// * `testnet` - specify the network, `true` if testnet else `false` for mainnet
///
pub fn key_recover(private_key: &PrivateKey, testnet: bool) -> Result<ExtendedKey, SignerError> {
    let secret_key = libsecp256k1::SecretKey::parse_slice(&private_key.0)?;
    let public_key = libsecp256k1::PublicKey::from_secret_key(&secret_key);
    let mut address = Address::new_secp256k1(&public_key.serialize())?;

    if testnet {
        address.set_network(Network::Testnet);
    } else {
        address.set_network(Network::Mainnet);
    }

    Ok(ExtendedKey {
        private_key: PrivateKey(secret_key.serialize()),
        public_key: PublicKey::SECP256K1PublicKey(public_key),
        address: address.to_string(),
    })
}

/// Get extended key from BLS private key
///
/// # Arguments
///
/// * `private_key` - A `bls_signatures::PrivateKey`
/// * `testnet` - specify the network, `true` if testnet else `false` for mainnet
///
pub fn key_recover_bls(
    private_key: &PrivateKey,
    testnet: bool,
) -> Result<ExtendedKey, SignerError> {
    let sk = bls_signatures::PrivateKey::from_bytes(&private_key.0)?;

    let mut address = Address::new_bls(&sk.public_key().as_bytes())?;

    if testnet {
        address.set_network(Network::Testnet);
    } else {
        address.set_network(Network::Mainnet);
    }

    let mut secret_key = PrivateKey([0; SECRET_KEY_SIZE]);
    secret_key.0.copy_from_slice(&sk.as_bytes());

    Ok(ExtendedKey {
        private_key: secret_key,
        public_key: PublicKey::BLSPublicKey(sk.public_key()),
        address: address.to_string(),
    })
}

/// Serialize a transaction and return a CBOR hexstring.
///
/// # Arguments
///
/// * `message` - a filecoin message (aka transaction)
///
pub fn transaction_serialize(message: &Message) -> Result<Vec<u8>, SignerError> {
    let message_cbor = message.marshal_cbor()?;
    Ok(message_cbor)
}

/// Parse a CBOR hextring into a filecoin transaction (signed or unsigned).
///
/// # Arguments
///
/// * `hexstring` - the cbor hexstring to parse
/// * `testnet` - boolean value `true` if testnet or `false` for mainnet
///
pub fn transaction_parse(cbor: &[u8], testnet: bool) -> Result<MessageTxAPI, SignerError> {
    let message: MessageTx = from_slice(cbor)?;

    let message_tx_with_network = MessageTxNetwork {
        message_tx: MessageTxAPI::from(message),
        testnet,
    };

    let parsed_message = MessageTxAPI::try_from(message_tx_with_network)?;

    Ok(parsed_message)
}

fn transaction_sign_secp56k1_raw(
    message: &Message,
    private_key: &PrivateKey,
) -> Result<Signature, SignerError> {
    let secret_key = libsecp256k1::SecretKey::parse_slice(&private_key.0)?;
    let message_digest =
        libsecp256k1::Message::parse_slice(&utils::blake2b_256(&message.to_signing_bytes()))?;

    let (signature_rs, recovery_id) = libsecp256k1::sign(&message_digest, &secret_key);

    let mut sig = [0; 65];
    sig[..64].copy_from_slice(&signature_rs.serialize().to_vec());
    sig[64] = recovery_id.serialize();

    let signature = Signature::new_secp256k1(sig.to_vec());

    Ok(signature)
}

fn transaction_sign_bls_raw(
    message: &Message,
    private_key: &PrivateKey,
) -> Result<Signature, SignerError> {
    let sk = bls_signatures::PrivateKey::from_bytes(&private_key.0)?;
    let sig = sk.sign(message.to_signing_bytes());
    let signature = Signature::new_bls(sig.as_bytes());

    Ok(signature)
}

/// Sign a transaction and return a raw signature (RSV format).
///
/// # Arguments
///
/// * `message` - an unsigned filecoin message
/// * `private_key` - a `PrivateKey`
///
pub fn transaction_sign_raw(
    message: &Message,
    private_key: &PrivateKey,
) -> Result<Signature, SignerError> {
    // the `from` address protocol let us know which signing scheme to use
    let signature = match message.from.protocol() {
        fvm_shared::address::Protocol::Secp256k1 => {
            transaction_sign_secp56k1_raw(message, private_key)?
        }
        fvm_shared::address::Protocol::BLS => transaction_sign_bls_raw(message, private_key)?,
        _ => {
            return Err(SignerError::GenericString(
                "Unknown signing protocol".to_string(),
            ));
        }
    };

    Ok(signature)
}

/// Sign a transaction and return a signed message (message + signature).
///
/// # Arguments
///
/// * `message` - an unsigned filecoin message
/// * `private_key` - a `PrivateKey`
///
pub fn transaction_sign(
    message: &Message,
    private_key: &PrivateKey,
) -> Result<SignedMessage, SignerError> {
    let signature = transaction_sign_raw(message, private_key)?;

    let signed_message = SignedMessage {
        message: message.to_owned(),
        signature,
    };

    Ok(signed_message)
}

fn verify_secp256k1_signature(signature: &Signature, cbor: &Vec<u8>) -> Result<bool, SignerError> {
    let network = Network::Testnet;

    let signature_rs = libsecp256k1::Signature::parse_standard_slice(&signature.bytes[..64])?;
    let recovery_id = libsecp256k1::RecoveryId::parse(signature.bytes[64])?;

    // Should be default network here
    // FIXME: For now only testnet
    let tx = transaction_parse(cbor, network == Network::Testnet)?;

    // Decode the CBOR transaction hex string into CBOR transaction buffer
    let message_digest = utils::get_digest(cbor.as_ref())?;

    let blob_to_sign = libsecp256k1::Message::parse_slice(&message_digest)?;

    let public_key = libsecp256k1::recover(&blob_to_sign, &signature_rs, &recovery_id)?;
    let mut from = Address::new_secp256k1(public_key.serialize().as_ref())?;
    from.set_network(network);

    let tx_from = match tx {
        MessageTxAPI::Message(tx) => tx.from,
        MessageTxAPI::SignedMessage(tx) => tx.message.from,
    };
    let expected_from = from.to_string();

    // Compare recovered public key with the public key from the transaction
    if tx_from.to_string() != expected_from {
        return Ok(false);
    }

    Ok(libsecp256k1::verify(
        &blob_to_sign,
        &signature_rs,
        &public_key,
    ))
}

fn verify_bls_signature(signature: &Signature, cbor: &Vec<u8>) -> Result<bool, SignerError> {
    // TODO: need a function to extract from public key from cbor buffer directly
    let message = transaction_parse(cbor, true)?;
    let message = message.get_message();

    let pk = bls_signatures::PublicKey::from_bytes(&message.from.payload_bytes())?;

    let sig = bls_signatures::Signature::from_bytes(signature.bytes())?;

    let signing_bytes = message.to_signing_bytes();

    let result = pk.verify(sig, signing_bytes);

    Ok(result)
}

/// Verify a signature. Return a boolean.
///
/// # Arguments
///
/// * `signature` - RSV format signature or BLS signature
/// * `cbor_buffer` - the CBOR transaction to verify the signature against
///
pub fn verify_signature(signature: &Signature, cbor: &Vec<u8>) -> Result<bool, SignerError> {
    // TODO: pass signature.bytes instead of the full signature
    let result = match signature.sig_type {
        SignatureType::Secp256k1 => verify_secp256k1_signature(signature, cbor)?,
        SignatureType::BLS => verify_bls_signature(signature, cbor)?,
    };

    Ok(result)
}

fn extract_from_pub_key_from_message(
    cbor_message: &Vec<u8>,
) -> Result<bls_signatures::PublicKey, SignerError> {
    let message = transaction_parse(cbor_message, true)?;
    let unsigned_message_api = message.get_message();
    let pk = bls_signatures::PublicKey::from_bytes(&unsigned_message_api.from.payload_bytes())?;

    Ok(pk)
}

fn extract_bls_signing_bytes_from_message(cbor_message: &Vec<u8>) -> Result<Vec<u8>, SignerError> {
    let message = transaction_parse(cbor_message, true)?;
    let unsigned_message_api = message.get_message();

    Ok(unsigned_message_api.to_signing_bytes())
}

pub fn verify_aggregated_signature(
    signature: &Signature,
    cbor_messages: &[Vec<u8>],
) -> Result<bool, SignerError> {
    let sig = bls_signatures::Signature::from_bytes(signature.bytes())?;

    // Get public keys from message
    let tmp: Result<Vec<_>, SignerError> = cbor_messages
        .iter()
        .map(extract_from_pub_key_from_message)
        .collect();

    let pks = match tmp {
        Ok(public_keys) => public_keys,
        Err(_) => {
            return Err(SignerError::GenericString(
                "Invalid public key extracted from message".to_string(),
            ));
        }
    };

    // Hashes
    let tmp: Result<Vec<_>, SignerError> = cbor_messages
        .iter()
        .map(extract_bls_signing_bytes_from_message)
        .collect();

    let signing_bytes = match tmp {
        Ok(bytes) => bytes,
        Err(_) => {
            return Err(SignerError::GenericString(
                "An invalid message was provided".to_string(),
            ));
        }
    };

    let hashes = signing_bytes
        .par_iter()
        .map(|signing_bytes| bls_signatures::hash(signing_bytes.as_ref()))
        .collect::<Vec<_>>();

    Ok(bls_signatures::verify(&sig, &hashes, pks.as_slice()))
}

/// Utilitary function to serialize parameters of a message. Return a CBOR hexstring.
///
/// # Arguments
///
/// * `params` - Parameters to serialize

pub fn serialize_params(params: MessageParams) -> Result<Vec<u8>, SignerError> {
    let serialized_params = params.serialize()?;
    let message_cbor = serialized_params.bytes().to_vec();
    Ok(message_cbor)
}

/// Sign a voucher for payment channel
///
/// # Arguments
///
/// * `voucher_string` - Voucher as base64 string;
/// * `private_key` - Private key as base64 string;
///
pub fn sign_voucher(
    voucher_string: String,
    private_key: &PrivateKey,
) -> Result<String, SignerError> {
    let decoded_voucher = base64::decode(voucher_string)?;
    let mut voucher: paych::SignedVoucher = from_slice(&decoded_voucher)?;

    let secret_key = libsecp256k1::SecretKey::parse_slice(&private_key.0)?;

    let svb = voucher
        .signing_bytes()
        .map_err(|err| SignerError::GenericString(err.to_string()))?;
    let digest = utils::get_digest_voucher(&svb)?;

    let blob_to_sign = libsecp256k1::Message::parse_slice(&digest)?;

    let (signature_rs, recovery_id) = libsecp256k1::sign(&blob_to_sign, &secret_key);

    let mut sig = [0; 65];
    sig[..64].copy_from_slice(&signature_rs.serialize()[..]);
    sig[64] = recovery_id.serialize();

    voucher.signature = Some(Signature::new_secp256k1(sig.to_vec()));

    let binary_voucher = to_vec(&voucher)?;
    let cbor_voucher = base64::encode(binary_voucher);

    Ok(cbor_voucher)
}

/// Create a voucher for payment channel
///
/// # Arguments
///
/// * `payment_channel_address` - The payment channel address;
/// * `time_lock_min` - Time lock min;
/// * `time_lock_maax` - Time lock max;
/// * `amount` - Amount in the voucher;
/// * `lane` - Lane of the voucher;
/// * `nonce` - Next nonce of the voucher;
///
pub fn create_voucher(
    payment_channel_address: String,
    time_lock_min: i64,
    time_lock_max: i64,
    amount: String,
    lane: u64,
    nonce: u64,
    min_settle_height: i64,
) -> Result<String, SignerError> {
    let pch = fvm_shared::address::Address::from_str(&payment_channel_address)?;
    let amount = match fvm_shared::bigint::BigInt::parse_bytes(amount.as_bytes(), 10) {
        Some(value) => value,
        None => {
            return Err(SignerError::GenericString(
                "`amount` couldn't be parsed.".to_string(),
            ));
        }
    };

    let voucher = paych::SignedVoucher {
        channel_addr: pch,
        time_lock_min,
        time_lock_max,
        secret_pre_image: Vec::new(),
        extra: None,
        lane,
        nonce,
        amount,
        min_settle_height,
        merges: Vec::new(),
        signature: None,
    };

    let cbor_voucher = base64::encode(to_vec(&voucher)?);

    Ok(cbor_voucher)
}

/// Deserialize Params
///
/// # Arguments
///
/// * `params_b64_string` - The base64 params string;
/// * `actor_type` - The string that tell the actor type;
/// * `method` - Method for which we want to deserialize the params;
pub fn deserialize_params(
    params_b64_string: String,
    actor_type: String,
    method: u64,
) -> Result<MessageParams, SignerError> {
    let params_decode = base64::decode(params_b64_string)?;
    let serialized_params = RawBytes::new(params_decode);

    // Deserialize pre-FVM init actor
    if actor_type.as_str() == "init" {
        match FromPrimitive::from_u64(method) {
            Some(MethodInit::Exec) => {
                let params: ExecParams = RawBytes::deserialize(&serialized_params)?;
                return Ok(MessageParams::ExecParams(params));
            }
            _ => {
                return Err(SignerError::GenericString(
                    "Unknown method for init actor.".to_string(),
                ));
            }
        }
    }

    // Deserialize pre-FVM multisig actor
    if actor_type.as_str() == "multisig" {
        match FromPrimitive::from_u64(method) {
            Some(multisig::Method::Propose) => {
                let params = serialized_params.deserialize::<multisig::ProposeParams>()?;

                return Ok(MessageParams::ProposeParams(params));
            }
            Some(multisig::Method::Approve) | Some(multisig::Method::Cancel) => {
                let params = serialized_params.deserialize::<multisig::TxnIDParams>()?;

                return Ok(MessageParams::TxnIDParams(params));
            }
            Some(multisig::Method::AddSigner) => {
                let params = serialized_params.deserialize::<multisig::AddSignerParams>()?;

                return Ok(MessageParams::AddSignerParams(params));
            }
            Some(multisig::Method::RemoveSigner) => {
                let params = serialized_params.deserialize::<multisig::RemoveSignerParams>()?;

                return Ok(MessageParams::RemoveSignerParams(params));
            }
            Some(multisig::Method::SwapSigner) => {
                let params = serialized_params.deserialize::<multisig::SwapSignerParams>()?;

                return Ok(MessageParams::SwapSignerParams(params));
            }
            Some(multisig::Method::ChangeNumApprovalsThreshold) => {
                let params = serialized_params
                    .deserialize::<multisig::ChangeNumApprovalsThresholdParams>()?;

                return Ok(MessageParams::ChangeNumApprovalsThresholdParams(params));
            }
            Some(multisig::Method::LockBalance) => {
                let params = serialized_params.deserialize::<multisig::LockBalanceParams>()?;

                return Ok(MessageParams::LockBalanceParams(params));
            }
            _ => {
                return Err(SignerError::GenericString(
                    "Unknown method for multisig actor.".to_string(),
                ));
            }
        }
    }

    // Deserialize pre-FVM paymentchannel actor
    if actor_type.as_str() == "paymentchannel" {
        match FromPrimitive::from_u64(method) {
            Some(paych::Method::UpdateChannelState) => {
                let params: fil_actor_paych::UpdateChannelStateParams =
                    RawBytes::deserialize(&serialized_params)?;

                return Ok(MessageParams::UpdateChannelStateParams(params));
            }
            Some(paych::Method::Settle) | Some(paych::Method::Collect) => {
                /* Note : those method doesn't have params to decode */
                return Ok(MessageParams::MessageParamsSerialized("".to_string()));
            }
            _ => {
                return Err(SignerError::GenericString(
                    "Unknown method for paymentchannel actor.".to_string(),
                ));
            }
        }
    }

    Err(SignerError::GenericString(
        "Actor type not supported.".to_string(),
    ))
}

/// Deserialize Constructor Params
///
/// # Arguments
///
/// * `params_b64_string` - The base64 params string;
/// * `code_cid` - The string that tell the actor type which is being crated with this parameters;
pub fn deserialize_constructor_params(
    params_b64_string: String,
    code_cid: String,
) -> Result<MessageParams, SignerError> {
    let params_decode = base64::decode(params_b64_string)?;
    let serialized_params = RawBytes::new(params_decode);

    if code_cid.as_str() == "multisig" {
        let params = serialized_params.deserialize::<multisig::ConstructorParams>()?;
        return Ok(MessageParams::MultisigConstructorParams(params));
    }

    if code_cid.as_str() == "paymentchannel" {
        let params = serialized_params.deserialize::<paych::ConstructorParams>()?;
        return Ok(MessageParams::PaychConstructorParams(params.into()));
    }

    if code_cid.as_str() == "fil/1/multisig" {
        let deprecated_multisig_params = serialized_params.deserialize::<ConstructorParamsV1>()?;
        let params = multisig::ConstructorParams {
            signers: deprecated_multisig_params.signers,
            num_approvals_threshold: deprecated_multisig_params.num_approvals_threshold,
            unlock_duration: deprecated_multisig_params.unlock_duration,
            start_epoch: 0,
        };
        return Ok(MessageParams::MultisigConstructorParams(params));
    }

    Err(SignerError::GenericString(
        "Code CID not supported.".to_string(),
    ))
}

/// Verify Voucher signature
///
/// # Arguments
///
/// * `voucher_base64_string` - The voucher as a base64 string;
/// * `address_signer` - The address matching the private key that signed the voucher;
pub fn verify_voucher_signature(
    voucher_base64_string: String,
    address_signer: String,
) -> Result<bool, SignerError> {
    let decoded_voucher = base64::decode(voucher_base64_string)?;
    let signed_voucher: paych::SignedVoucher = from_slice(&decoded_voucher)?;

    let address = Address::from_str(&address_signer)?;

    let sv_bytes = signed_voucher
        .signing_bytes()
        .map_err(|err| SignerError::GenericString(err.to_string()))?;
    let digest = utils::get_digest_voucher(&sv_bytes)?;

    match &signed_voucher.signature {
        Some(signature) => match address.protocol() {
            Protocol::Secp256k1 => {
                let sig = libsecp256k1::Signature::parse_standard_slice(&signature.bytes()[..64])?;
                let recovery_id = libsecp256k1::RecoveryId::parse(signature.bytes()[64])?;
                let message = libsecp256k1::Message::parse(&digest);
                let public_key = libsecp256k1::recover(&message, &sig, &recovery_id)?;
                let mut signer = Address::new_secp256k1(public_key.serialize().as_ref())?;
                signer.set_network(address.network());

                if signer.to_string() != address.to_string() {
                    Err(SignerError::GenericString(
                        "Address recovered doesn't match address given".to_string(),
                    ))
                } else {
                    Ok(libsecp256k1::verify(&message, &sig, &public_key))
                }
            }
            Protocol::BLS => {
                let pk = bls_signatures::PublicKey::from_bytes(&address.payload_bytes())?;
                let sig = bls_signatures::Signature::from_bytes(signature.bytes())?;

                Ok(pk.verify(sig, digest))
            }
            _ => Err(SignerError::GenericString(
                "Address should BLS or Secp256k1.".to_string(),
            )),
        },
        None => Err(SignerError::GenericString(
            "Voucher not signed.".to_string(),
        )),
    }
}

/// Serialize voucher
///
/// # Arguments
///
/// * `voucher` - The voucher data;
pub fn serialize_voucher(voucher: SignedVoucherWrapper) -> Result<String, SignerError> {
    let serialized_voucher = to_vec(&voucher)?;

    Ok(base64::encode(serialized_voucher))
}

/// Deserialize voucher
///
/// # Arguments
///
/// * `voucher_base64_string` - The voucher data;
pub fn deserialize_voucher(
    voucher_base64_string: String,
) -> Result<SignedVoucherWrapper, SignerError> {
    let serialized_voucher = base64::decode(&voucher_base64_string)?;

    let signed_voucher: paych::SignedVoucher = from_slice(&serialized_voucher)?;

    Ok(SignedVoucherWrapper(signed_voucher))
}

/// Compute proposal hash
///
/// # Arguments
///
/// * `proposal_hash_data` - The proposal hash;
pub fn compute_proposal_hash(
    proposal_hash_data: ProposalHashDataAPI,
) -> Result<String, SignerError> {
    let proposal_data = multisig::ProposalHashData {
        requester: proposal_hash_data.requester.as_ref(),
        to: &proposal_hash_data.to,
        value: &proposal_hash_data.value,
        method: &proposal_hash_data.method,
        params: &proposal_hash_data.params,
    };

    let serialize_proposal_data = RawBytes::serialize(proposal_data)
        .map_err(|err| SignerError::GenericString(err.to_string()))?;
    let proposal_hash = utils::blake2b_256(&serialize_proposal_data);

    Ok(base64::encode(proposal_hash))
}

/// Return the CID of a message
///
/// # Arguments
///
/// * `message_api` - The message;
pub fn get_cid(message_api: MessageTxAPI) -> Result<String, SignerError> {
    match message_api {
        MessageTxAPI::Message(message) => {
            let cid = message.cid()?;

            Ok(cid.to_string())
        }
        MessageTxAPI::SignedMessage(signed_message) => {
            let cid = signed_message.cid()?;

            Ok(cid.to_string())
        }
    }
}
