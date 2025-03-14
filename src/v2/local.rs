//! An Implementation of Paseto V2 "local" tokens (or tokens that are encrypted with a shared secret).

use crate::errors::{GenericError, SodiumErrors};
use crate::pae::pae;

use base64::{decode_config, encode_config, URL_SAFE_NO_PAD};
use failure::Error;
use ring::constant_time::verify_slices_are_equal as ConstantTimeEquals;
use ring::rand::{SecureRandom, SystemRandom};
use sodiumoxide::crypto::aead::xchacha20poly1305_ietf::{open as Decrypt, seal as Encrypt, Key, Nonce};
use sodiumoxide::crypto::generichash::State as GenericHashState;

const HEADER: &str = "v2.local.";

/// Encrypt a "v2.local" paseto token.
///
/// Keys must be exactly 32 bytes long, this is a requirement of the underlying
/// algorithim.
///
/// Returns a result of a string if encryption was successful.
pub fn local_paseto(msg: &str, footer: Option<&str>, key: &[u8]) -> Result<String, Error> {
  let rng = SystemRandom::new();
  let mut buff: [u8; 24] = [0u8; 24];
  let res = rng.fill(&mut buff);
  if res.is_err() {
    return Err(GenericError::RandomError {})?;
  }

  if key.len() != 32 {
    return Err(SodiumErrors::InvalidKeySize {
      size_needed: 32,
      size_provided: key.len(),
    })?;
  }

  underlying_local_paseto(msg, footer, &buff, key)
}

/// Performs the underlying encryption of a paseto token. Split for unit testing.
///
/// `msg` - The Msg to Encrypt.
/// `footer` - The footer to add.
/// `nonce_key` - The key to the nonce, should be securely generated.
/// `key` - The key to encrypt the message with.
fn underlying_local_paseto(msg: &str, footer: Option<&str>, nonce_key: &[u8; 24], key: &[u8]) -> Result<String, Error> {
  let footer_frd = footer.unwrap_or("");

  if let Ok(mut state) = GenericHashState::new(Some(24), Some(nonce_key)) {
    if let Ok(_) = state.update(msg.as_bytes()) {
      if let Ok(finalized) = state.finalize() {
        let nonce_bytes = finalized.as_ref();
        if let Some(nonce) = Nonce::from_slice(nonce_bytes) {
          let key_obj = Key::from_slice(key);
          if key_obj.is_none() {
            return Err(SodiumErrors::InvalidKey {})?;
          }
          let key_obj = key_obj.unwrap();

          let pre_auth = pae(&[HEADER.as_bytes(), &nonce_bytes, footer_frd.as_bytes()]);

          let crypted = Encrypt(msg.as_bytes(), Some(pre_auth.as_ref()), &nonce, &key_obj);

          let mut n_and_c = Vec::new();
          n_and_c.extend_from_slice(&nonce_bytes);
          n_and_c.extend_from_slice(&crypted);

          let token = if !footer_frd.is_empty() {
            format!(
              "{}{}.{}",
              HEADER,
              encode_config(&n_and_c, URL_SAFE_NO_PAD),
              encode_config(footer_frd.as_bytes(), URL_SAFE_NO_PAD)
            )
          } else {
            format!("{}{}", HEADER, encode_config(&n_and_c, URL_SAFE_NO_PAD))
          };

          Ok(token)
        } else {
          Err(SodiumErrors::FunctionError {})?
        }
      } else {
        Err(SodiumErrors::FunctionError {})?
      }
    } else {
      Err(SodiumErrors::FunctionError {})?
    }
  } else {
    Err(SodiumErrors::FunctionError {})?
  }
}

/// Decrypt a Paseto TOKEN, validating against an optional footer.
///
/// `token`: The Token to decrypt.
/// `footer`: The Optional footer to validate.
/// `key`: The key to decrypt your Paseto.
pub fn decrypt_paseto(token: &str, footer: Option<&str>, key: &[u8]) -> Result<String, Error> {
  let token_parts = token.split(".").collect::<Vec<_>>();
  if token_parts.len() < 3 {
    return Err(GenericError::InvalidToken {})?;
  }

  let is_footer_some = footer.is_some();
  let footer_str = footer.unwrap_or("");

  if is_footer_some {
    if token_parts.len() < 4 {
      return Err(GenericError::InvalidFooter {})?;
    }
    let as_base64 = encode_config(footer_str.as_bytes(), URL_SAFE_NO_PAD);

    if ConstantTimeEquals(as_base64.as_bytes(), token_parts[3].as_bytes()).is_err() {
      return Err(GenericError::InvalidFooter {})?;
    }
  }

  if token_parts[0] != "v2" || token_parts[1] != "local" {
    return Err(GenericError::InvalidToken {})?;
  }

  let mut decoded = decode_config(token_parts[2].as_bytes(), URL_SAFE_NO_PAD)?;
  let (nonce, ciphertext) = decoded.split_at_mut(24);

  let pre_auth = pae(&[HEADER.as_bytes(), nonce, footer_str.as_bytes()]);

  let nonce_obj = Nonce::from_slice(nonce);
  let key_obj = Key::from_slice(key);
  if nonce_obj.is_none() || key_obj.is_none() {
    return Err(SodiumErrors::InvalidKey {})?;
  }
  let nonce_obj = nonce_obj.unwrap();
  let key_obj = key_obj.unwrap();

  let decrypted = Decrypt(ciphertext, Some(&pre_auth), &nonce_obj, &key_obj);
  if decrypted.is_err() {
    return Err(SodiumErrors::FunctionError {})?;
  }
  let decrypted = decrypted.unwrap();

  Ok(String::from_utf8(decrypted)?)
}

#[cfg(test)]
mod unit_tests {
  use super::*;

  #[test]
  fn paseto_empty_encrypt_verify() {
    let empty_key = [0; 32];
    let full_key = [255; 32];
    let result = underlying_local_paseto("", None, &[0; 24], &empty_key);
    if result.is_err() {
      println!("Failed to encrypt Paseto!");
      println!("{:?}", result);
      panic!("Paseto Failure Encryption!");
    }
    let the_str = result.unwrap();

    assert_eq!(
      "v2.local.driRNhM20GQPvlWfJCepzh6HdijAq-yNUtKpdy5KXjKfpSKrOlqQvQ",
      the_str
    );

    let result_full = underlying_local_paseto("", None, &[0; 24], &full_key);
    if result_full.is_err() {
      println!("Failed to encrypt Paseto!");
      println!("{:?}", result_full);
      panic!("Paseto Failure Encryption!");
    }
    let the_full_str = result_full.unwrap();

    assert_eq!(
      "v2.local.driRNhM20GQPvlWfJCepzh6HdijAq-yNSOvpveyCsjPYfe9mtiJDVg",
      the_full_str
    );
  }

  #[test]
  fn paseto_non_empty_footer_encrypt_verify() {
    let empty_key = [0; 32];
    let full_key = [255; 32];

    let result = underlying_local_paseto("", Some("Cuon Alpinus"), &[0; 24], &empty_key);
    if result.is_err() {
      println!("Failed to encrypt Paseto!");
      println!("{:?}", result);
      panic!("Paseto Failure Encryption!");
    }
    let the_str = result.unwrap();

    assert_eq!(
      "v2.local.driRNhM20GQPvlWfJCepzh6HdijAq-yNfzz6yGkE4ZxojJAJwKLfvg.Q3VvbiBBbHBpbnVz",
      the_str
    );

    let full_result = underlying_local_paseto("", Some("Cuon Alpinus"), &[0; 24], &full_key);
    if full_result.is_err() {
      println!("Failed to encrypt Paseto!");
      println!("{:?}", full_result);
      panic!("Paseto Failure Encryption!");
    }
    let full_str = full_result.unwrap();

    assert_eq!(
      "v2.local.driRNhM20GQPvlWfJCepzh6HdijAq-yNJbTJxAGtEg4ZMXY9g2LSoQ.Q3VvbiBBbHBpbnVz",
      full_str
    );
  }

  #[test]
  fn paseto_non_empty_msg_encrypt_verify() {
    let empty_key = [0; 32];
    let full_key = [255; 32];

    let result = underlying_local_paseto("Love is stronger than hate or fear", None, &[0; 24], &empty_key);
    if result.is_err() {
      println!("Failed to encrypt Paseto!");
      println!("{:?}", result);
      panic!("Paseto Failure Encryption!");
    }
    let the_str = result.unwrap();

    assert_eq!(
      "v2.local.BEsKs5AolRYDb_O-bO-lwHWUextpShFSvu6cB-KuR4wR9uDMjd45cPiOF0zxb7rrtOB5tRcS7dWsFwY4ONEuL5sWeunqHC9jxU0",
      the_str
    );

    let full_result = underlying_local_paseto("Love is stronger than hate or fear", None, &[0; 24], &full_key);
    if full_result.is_err() {
      println!("Failed to encrypt Paseto!");
      println!("{:?}", full_result);
      panic!("Paseto Failure Encryption!");
    }

    let full_str = full_result.unwrap();

    assert_eq!(
      "v2.local.BEsKs5AolRYDb_O-bO-lwHWUextpShFSjvSia2-chHyMi4LtHA8yFr1V7iZmKBWqzg5geEyNAAaD6xSEfxoET1xXqahe1jqmmPw",
      full_str
    );
  }

  #[test]
  fn full_round_paseto() {
    let empty_key = [0; 32];

    let result = local_paseto("Love is stronger than hate or fear", Some("gwiz-bot"), &empty_key);
    if result.is_err() {
      println!("Failed to encrypt Paseto!");
      println!("{:?}", result);
      panic!("Paseto Failure Encryption!");
    }
    let the_str = result.unwrap();

    println!("Paseto Full Round Token: [ {:?} ]", the_str);

    let decrypted_result = decrypt_paseto(&the_str, Some("gwiz-bot"), &empty_key);
    if decrypted_result.is_err() {
      println!("Failed to decrypt Paseto!");
      println!("{:?}", decrypted_result);
      panic!("Paseto Failure Decryption!");
    }
    let decrypted = decrypted_result.unwrap();

    assert_eq!(decrypted, "Love is stronger than hate or fear");
  }
}
