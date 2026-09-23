import com.example.db.UserRepository as Repo
import com.example.util.*

expect fun platformName(): String

val String.lastChar: Char
    get() = this[length - 1]

fun String.shout(): String = this.uppercase()

val listener: (String) -> Unit = {}

class Button(val onClick: () -> Unit, label: String)

internal class Cache(private val size: Int, internal val tag: String) {
    internal val entries = mutableMapOf<String, Int>()

    var balance: Int = 0
        get() = audit(field)
        set(value) {
            record(value)
            field = value
        }

    fun audit(v: Int): Int = v
    fun record(v: Int) {}

    private companion object Factory {
        fun create(): Cache {
            val local = Cache(1, "x")
            return local
        }
    }
}

data class Vec(val x: Int) {
    operator fun plus(other: Vec): Vec {
        return combine(other)
    }

    fun combine(other: Vec): Vec = Vec(x + other.x)
}

class Builder {
    fun append(s: String) {}
}

class Outer {
    fun Builder.ext() {
        this.append("x")
    }
}

private enum class Mode { FAST, SLOW }

enum class Op {
    ADD {
        override fun apply(a: Int, b: Int) = a + b
    };

    abstract fun apply(a: Int, b: Int): Int
}

fun outer(x: Int): Int {
    fun helper(y: Int) = y * 2
    return helper(x)
}

fun check(flag: Boolean): Any? {
    val kind = Widget::class
    if (flag == true) return null
    return false
}

interface UserRepo : JpaRepository<User, Long> {
    @Query("SELECT u FROM User u WHERE u.name = :name")
    fun findByName(name: String): List<User>
}

class Dao(private val jdbc: JdbcTemplate) {
    fun load() {
        jdbc.query("SELECT * FROM accounts", mapper)
        val uri = URI.create("https://api.example.com/health")
    }
}

class Config {
    fun setup(r: Registry) {
        r.apply {
            context("admin") {
                register("x")
            }
        }
    }
}
