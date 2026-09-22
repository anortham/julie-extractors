struct device;
struct driver { const char *name; int (*probe)(struct device *dev); };

/** Duplicate a string. */
char *str_dup(const char *s);
struct Node *node_new(void);

char **make_list(int n) {
    return calloc(n, sizeof(char *));
}

typedef struct Buffer { int len; } Buffer;
static Buffer *global_buf;
const char *const names[3];
int count, *ptr, arr[4];

typedef struct { int x; } Foo, *FooPtr;
typedef void handler_t(int signo);
typedef int (*visit_fn)(Buffer *buffer, void *ctx);
typedef unsigned long size_type;
typedef enum { RED, GREEN } Color;
typedef union { int i; float f; } Number;

static int compare(const void *a, const void *b) { return 0; }
static void (*handlers[4])(int);
int (*global_cmp)(const void *, const void *) = compare;

int use(struct driver *drv, visit_fn visit) {
    Buffer *b = global_buf;
    int (*cmp)(const void *, const void *) = compare;
    global_cmp(b, ptr);
    return cmp(b, names) + drv->probe(0) + visit(b, 0);
}
