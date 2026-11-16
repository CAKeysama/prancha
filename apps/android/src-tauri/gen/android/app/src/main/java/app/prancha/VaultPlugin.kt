package app.prancha

import android.app.Activity
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class VaultArgs {
  /** Bytes em hexadecimal. */
  lateinit var data: String
}

/**
 * O cofre do Android para os tokens do MyAnimeList: AES-GCM com uma chave do Android Keystore, que não sai do
 * aparelho nem vai no backup. O Rust (crates/prancha-app/src/android.rs) guarda só o resultado cifrado no banco.
 */
@TauriPlugin
class VaultPlugin(activity: Activity) : Plugin(activity) {
  private fun key(): SecretKey {
    val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
    (store.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
    generator.init(
      KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build()
    )
    return generator.generateKey()
  }

  /** O vetor inicial (12 bytes, sorteado pelo Keystore) vai na frente do texto cifrado. */
  @Command
  fun seal(invoke: Invoke) {
    try {
      val plain = fromHex(invoke.parseArgs(VaultArgs::class.java).data)
      val cipher = Cipher.getInstance(TRANSFORMATION)
      cipher.init(Cipher.ENCRYPT_MODE, key())
      invoke.resolve(JSObject().put("data", toHex(cipher.iv + cipher.doFinal(plain))))
    } catch (e: Exception) {
      invoke.reject("O cofre do Android não cifrou: $e")
    }
  }

  @Command
  fun unseal(invoke: Invoke) {
    try {
      val sealed = fromHex(invoke.parseArgs(VaultArgs::class.java).data)
      if (sealed.size <= IV_SIZE) throw IllegalArgumentException("dado curto demais")
      val cipher = Cipher.getInstance(TRANSFORMATION)
      cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, sealed, 0, IV_SIZE))
      invoke.resolve(JSObject().put("data", toHex(cipher.doFinal(sealed, IV_SIZE, sealed.size - IV_SIZE))))
    } catch (e: Exception) {
      invoke.reject("O cofre do Android não abriu: $e")
    }
  }

  private fun toHex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

  private fun fromHex(text: String): ByteArray {
    if (text.length % 2 != 0) throw IllegalArgumentException("hexadecimal inválido")
    return ByteArray(text.length / 2) { text.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
  }

  companion object {
    private const val KEYSTORE = "AndroidKeyStore"
    private const val ALIAS = "prancha-mal"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val IV_SIZE = 12
  }
}
