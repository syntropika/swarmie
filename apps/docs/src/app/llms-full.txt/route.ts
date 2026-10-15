import { docsLlms } from "~/src/lib/source";

export const revalidate = false;

export async function GET() {
  return new Response(await docsLlms.full());
}
