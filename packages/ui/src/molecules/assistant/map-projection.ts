/** Valid geographic coordinates accepted by a host map adapter. */
export function isMapCoordinate(latitude: number, longitude: number) {
  return (
    Number.isFinite(latitude) &&
    Number.isFinite(longitude) &&
    latitude >= -90 &&
    latitude <= 90 &&
    longitude >= -180 &&
    longitude <= 180
  );
}

/** Fit an illustrative equirectangular viewport to valid places without requesting map tiles. */
export function projectMapPlaces<T extends { id: string; latitude: number; longitude: number }>(
  places: readonly T[],
) {
  const valid = places.filter((place) => isMapCoordinate(place.latitude, place.longitude));
  if (valid.length === 0) {
    return [];
  }
  const latitudes = valid.map((place) => place.latitude);
  const longitudes = valid.map((place) => place.longitude);
  const north = Math.max(...latitudes);
  const south = Math.min(...latitudes);
  const west = Math.min(...longitudes);
  const east = Math.max(...longitudes);
  return valid.map((place) => ({
    ...place,
    x: east === west ? 300 : 48 + ((place.longitude - west) / (east - west)) * 504,
    y: north === south ? 120 : 40 + ((north - place.latitude) / (north - south)) * 160,
  }));
}
