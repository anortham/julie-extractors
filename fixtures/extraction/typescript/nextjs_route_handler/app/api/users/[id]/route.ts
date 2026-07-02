import { NextResponse } from "next/server";

export async function GET(
    request: Request,
    { params }: { params: { id: string } },
): Promise<Response> {
    return NextResponse.json({ id: params.id });
}

export const DELETE = async (): Promise<Response> => {
    return new Response(null, { status: 204 });
};
