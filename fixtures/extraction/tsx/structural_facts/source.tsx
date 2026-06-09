export async function View() {
    const data = await load();
    return <div>{data}</div>;
}
