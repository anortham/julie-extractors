interface Props {
  endpoint: string;
}

export function SaveButton({ endpoint }: Props) {
  return (
    <button hx-put="/todos/1" data-hx-patch="/todos/1" hx-get={endpoint}>
      Save
    </button>
  );
}
