package eu.kanade.tachiyomi.network

import android.content.Context
import eu.kanade.tachiyomi.network.interceptor.UncaughtExceptionInterceptor
import eu.kanade.tachiyomi.network.interceptor.UserAgentInterceptor
import okhttp3.Cache
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import okhttp3.Response
import java.io.File
import java.io.IOException
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds

/**
 * Adaptado do NetworkHelper do Mihon para o Prancha: sem injeção de dependências nem preferências de rede, e sem
 * o resolvedor automático do Cloudflare. Como no computador, a verificação anti-robô é feita pela pessoa, numa tela
 * do site (VerifyActivity), e os cookies dela ficam no CookieManager que o [cookieJar] lê.
 */
class NetworkHelper(context: Context) {

    val cookieJar = AndroidCookieJar()

    val client: OkHttpClient by lazy {
        OkHttpClient.Builder()
            .cookieJar(cookieJar)
            .connectTimeout(30.seconds)
            .readTimeout(30.seconds)
            .callTimeout(2.minutes)
            .cache(Cache(directory = File(context.cacheDir, "mihon_rede"), maxSize = 5L * 1024 * 1024))
            .addInterceptor(UncaughtExceptionInterceptor())
            .addInterceptor(UserAgentInterceptor(::defaultUserAgentProvider))
            .addInterceptor(CloudflareCheck())
            .build()
    }

    /**
     * @deprecated Since extension-lib 1.5
     */
    @Deprecated("The regular client handles Cloudflare by default")
    @Suppress("UNUSED")
    val cloudflareClient: OkHttpClient get() = client

    fun defaultUserAgentProvider() = USER_AGENT

    companion object {
        /** O mesmo navegador da tela de verificação: a liberação de um site vale só para quem passou por ela. */
        @Volatile
        var USER_AGENT = "Mozilla/5.0 (Linux; Android 14; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36"
    }
}

/** A página de desafio do Cloudflare vira um erro com o nome dele, que o núcleo traduz para "verificação anti-robô". */
private class CloudflareCheck : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val response = chain.proceed(chain.request())
        if (response.header("cf-mitigated") == "challenge" && response.header("Server") in setOf("cloudflare-nginx", "cloudflare")) {
            response.close()
            throw IOException("Cloudflare: verificação anti-robô pedida por ${chain.request().url.host}")
        }
        return response
    }
}
