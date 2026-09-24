typedef struct Result { int x; } Result;
Result make(void) { Result x; return x; }
void use(void) { __auto_type made = make(); }
