import sys
import json
from astropy.io import fits

def validate(filename):
    result = []
    try:
        with fits.open(filename) as hdul:
            for i, hdu in enumerate(hdul):
                # Clean up header for JSON serialization
                header_dict = {}
                for key, value in hdu.header.items():
                    if key == '': continue # Skip blank keys
                    if isinstance(value, (str, int, float, bool)) or value is None:
                        header_dict[key] = value
                    else:
                        header_dict[key] = str(value)

                info = {
                    "index": i,
                    "type": hdu.__class__.__name__,
                    "header": header_dict,
                    "data_shape": hdu.data.shape if hdu.data is not None else None,
                    "data_type": str(hdu.data.dtype) if hdu.data is not None else None,
                }
                result.append(info)
        return result
    except Exception as e:
        return {"error": str(e)}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python validate_metadata.py <file.fits>")
        sys.exit(1)

    filename = sys.argv[1]
    data = validate(filename)
    print(json.dumps(data, indent=2))
