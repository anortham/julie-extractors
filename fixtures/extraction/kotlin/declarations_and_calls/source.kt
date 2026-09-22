package com.example.orders

/** Dev-only beans. */
@Component
@Profile("dev")
class DevConfig {
    fun bean() {}
}

/** Deprecated helper. */
@Deprecated("use other")
fun old() {}

@Keep
enum class Mode { A, B }

@Retention(AnnotationRetention.RUNTIME)
@Target(AnnotationTarget.FUNCTION)
annotation class FromJson

interface Repository<T> {
    fun find(id: Int): T?
}

abstract class BaseViewModel<S>

class User

class UserRepo : Repository<User> {
    override fun find(id: Int): User? = null
}

class UserVm : BaseViewModel<String>()

class StringAdapter : JsonAdapter<String>(), JsonAdapter.Factory, Comparable<StringAdapter>, Handler by inner

sealed class LoginState {
    object Loading : LoginState()
}

sealed class ProfileState {
    object Loading : ProfileState()
}

val appModule = module { single { createRepo() } }
private val config by lazy { loadConfig() }
val greeting: String
    get() = buildGreeting()

fun createRepo(): Any = Any()
fun loadConfig(): String = ""
fun buildGreeting(): String = ""

class Money(val cents: Long) {
    infix fun plusCents(other: Long): Money = Money(cents + other)
}

class TypeName(val simpleName: String)
private fun TypeName.toVariableName(): String = simpleName.lowercase()
fun String?.orBlank(): String = this ?: ""
val TypeName.display: String get() = simpleName

fun isValid(m: Money): Boolean = m.cents > 0

fun abs(a: Int) = if (a > 0) a else -a

fun render(result: ApiResult, type: TypeName, s: String?, items: List<Money>, a: Money): String {
    val c = a plusCents 5
    val valid = items.filter(::isValid)
    val server = embeddedServer(Netty).start(wait = true)
    val entry: Map.Entry<String, Int>? = null
    try {
        println(c)
    } catch (e: java.io.IOException) {
    }
    return when (result) {
        is ApiResult.Success -> type.toVariableName() + s.orBlank()
        else -> ""
    }
}
