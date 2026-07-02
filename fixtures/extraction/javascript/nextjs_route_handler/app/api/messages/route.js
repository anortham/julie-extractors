export async function GET() {
    return Response.json({ items: [] });
}

export const POST = async (request) => {
    const body = await request.json();
    return Response.json(body, { status: 201 });
};

// Non-verb exports and helpers stay silent.
export function serialize(value) {
    return JSON.stringify(value);
}
