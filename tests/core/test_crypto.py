"""Tests for ltbox.crypto – happy path and edge cases."""

import hashlib
import os
import struct

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.primitives.padding import PKCS7

from ltbox import crypto


def _build_encrypted_file(plaintext: bytes, password: str = "OSD") -> bytes:
    """Build an encrypted payload matching crypto.decrypt_file's expected format.

    Layout: IV (16) | salt (16) | AES-CBC(header + body + digest + padding)
    Header: original_size (8 LE) | signature (8)
    """
    iv = os.urandom(16)
    salt = os.urandom(16)

    key = crypto.PBKDF1(password, salt, 32, hashlib.sha256, 1000)
    digest = hashlib.sha256(plaintext).digest()
    signature = b"\xcf\x06\x05\x04\x03\x02\x01\xfc"
    header = struct.pack("<q", len(plaintext)) + signature
    raw = header + plaintext + digest

    padder = PKCS7(128).padder()
    padded = padder.update(raw) + padder.finalize()

    cipher = Cipher(algorithms.AES(key), modes.CBC(iv))
    encrypted = cipher.encryptor().update(padded) + cipher.encryptor().finalize()

    # Redo encryption properly (single encryptor instance)
    encryptor = cipher.encryptor()
    encrypted = encryptor.update(padded) + encryptor.finalize()

    return iv + salt + encrypted


class TestDecryptFile:
    def test_happy_path_round_trip(self, tmp_path):
        original = b"Hello, LTBox firmware decryption test!"
        enc_path = tmp_path / "test.enc"
        dec_path = tmp_path / "test.dec"

        enc_path.write_bytes(_build_encrypted_file(original))

        from unittest.mock import patch

        with patch("ltbox.utils.ui"):
            result = crypto.decrypt_file(str(enc_path), str(dec_path))

        assert result is True
        assert dec_path.read_bytes() == original

    def test_large_payload_round_trip(self, tmp_path):
        original = os.urandom(64 * 1024)
        enc_path = tmp_path / "large.enc"
        dec_path = tmp_path / "large.dec"

        enc_path.write_bytes(_build_encrypted_file(original))

        from unittest.mock import patch

        with patch("ltbox.utils.ui"):
            result = crypto.decrypt_file(str(enc_path), str(dec_path))

        assert result is True
        assert dec_path.read_bytes() == original

    def test_bad_signature_returns_false(self, tmp_path):
        enc_path = tmp_path / "bad_sig.enc"
        dec_path = tmp_path / "bad_sig.dec"

        # Build valid, then corrupt the encrypted body so signature check fails
        enc_path.write_bytes(b"\x00" * 32 + b"junk_encrypted_body_padding_here")

        from unittest.mock import patch

        with patch("ltbox.utils.ui"):
            result = crypto.decrypt_file(str(enc_path), str(dec_path))

        assert result is False

    def test_missing_file_returns_false(self, tmp_path):
        from unittest.mock import patch

        with patch("ltbox.utils.ui"):
            result = crypto.decrypt_file(
                str(tmp_path / "nonexistent.enc"), str(tmp_path / "out.dec")
            )

        assert result is False

    def test_corrupted_digest_returns_false(self, tmp_path):
        """Tamper with body after encryption to cause digest mismatch."""
        iv = os.urandom(16)
        salt = os.urandom(16)
        key = crypto.PBKDF1("OSD", salt, 32, hashlib.sha256, 1000)

        body = b"authentic content"
        signature = b"\xcf\x06\x05\x04\x03\x02\x01\xfc"
        header = struct.pack("<q", len(body)) + signature

        # Use wrong digest
        bad_digest = hashlib.sha256(b"tampered").digest()
        raw = header + body + bad_digest

        padder = PKCS7(128).padder()
        padded = padder.update(raw) + padder.finalize()

        cipher = Cipher(algorithms.AES(key), modes.CBC(iv))
        encryptor = cipher.encryptor()
        encrypted = encryptor.update(padded) + encryptor.finalize()

        enc_path = tmp_path / "bad_digest.enc"
        dec_path = tmp_path / "bad_digest.dec"
        enc_path.write_bytes(iv + salt + encrypted)

        from unittest.mock import patch

        with patch("ltbox.utils.ui"):
            result = crypto.decrypt_file(str(enc_path), str(dec_path))

        assert result is False
